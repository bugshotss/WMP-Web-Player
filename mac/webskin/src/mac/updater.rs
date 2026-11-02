//! The update checker: once per start, a build that knows its repository (the release workflow writes it
//! into Info.plist) asks GitHub for the latest release. A newer one shows as "Update available" in full
//! mode and the menus. Install Now downloads the release's Mac zip, checks it against GitHub's checksum,
//! replaces this copy of the app, and starts the new one; What's New opens the release page.

use super::{message_box, mtm, open_url};
use crate::net::{Download, Progress};
use crate::skin::{AppAction, MenuItem};
use crate::update::{self, UpdateInfo};
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication};
use objc2_foundation::{NSBundle, NSString};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The repository updates come from when a build doesn't name one (Check for Updates… in a local build).
const DEFAULT_REPOSITORY: &str = "https://github.com/bugshotss/WMP-Web-Player";

type Answer = Arc<Mutex<Option<Result<Option<UpdateInfo>, String>>>>;

#[derive(Default)]
struct Updater {
    available: Option<UpdateInfo>,
    /// A check under way, and whether the user asked for it (then the answer is shown either way).
    checking: Option<(Answer, bool)>,
    download: Option<(UpdateInfo, Download)>,
}

thread_local! {
    static UPDATER: RefCell<Updater> = RefCell::default();
}

fn info_string(key: &str) -> Option<String> {
    let bundle = NSBundle::mainBundle();
    bundle
        .objectForInfoDictionaryKey(&NSString::from_str(key))
        .and_then(|v| v.downcast::<NSString>().ok())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// This copy's version: the app bundle's, or the crate's when run with cargo (WEBSKIN_VERSION, a test hook,
/// stands in for either).
pub fn version() -> String {
    std::env::var("WEBSKIN_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| info_string("CFBundleShortVersionString"))
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// The releases feed to check: WEBSKIN_UPDATES (a test hook), or the repository the build came from.
fn feed(manual: bool) -> Option<String> {
    if let Some(f) = std::env::var("WEBSKIN_UPDATES").ok().filter(|f| !f.is_empty()) {
        return Some(f);
    }
    let repo = info_string("WebSkinRepository").or_else(|| manual.then(|| DEFAULT_REPOSITORY.to_string()))?;
    update::feed_for(&repo)
}

fn begin_check(manual: bool) {
    let Some(feed) = feed(manual) else { return };
    if UPDATER.with(|u| u.borrow().checking.is_some()) {
        return;
    }
    let answer: Answer = Arc::default();
    let out = answer.clone();
    let current = version();
    std::thread::spawn(move || {
        let result = update::check(&feed, &current);
        *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
    });
    UPDATER.with(|u| u.borrow_mut().checking = Some((answer, manual)));
}

/// Checks once at start, when the build knows where it came from.
pub fn start() {
    // An earlier update's leftovers have done their job.
    let _ = std::fs::remove_dir_all(work_dir());
    begin_check(false);
}

pub fn available() -> Option<UpdateInfo> {
    UPDATER.with(|u| u.borrow().available.clone())
}

/// "Downloading 40%…" while Install Now runs.
pub fn status() -> Option<String> {
    UPDATER.with(|u| {
        let u = u.borrow();
        let (info, d) = u.download.as_ref()?;
        Some(match d.progress() {
            Progress::Running { bytes } if info.size > 0 => format!("Downloading {}%…", (bytes * 100 / info.size).min(100)),
            _ => "Downloading…".to_string(),
        })
    })
}

/// "Install Update x.y.z…" and "What's New in x.y.z" once a newer release is out.
pub fn menu_entries() -> Vec<(MenuItem, AppAction)> {
    let Some(u) = available() else { return vec![] };
    let busy = status().is_some();
    vec![
        (
            MenuItem::item(&status().unwrap_or_else(|| format!("Install Update {}…", u.version)), !busy, false),
            AppAction::Host("update:install".into()),
        ),
        (MenuItem::item(&format!("What's New in {}", u.version), true, false), AppAction::Host("update:whatsnew".into())),
    ]
}

pub fn command(c: &str) {
    match c {
        "update:check" => begin_check(true),
        "update:install" => ask_to_install(),
        "update:whatsnew" => {
            if let Some(u) = available() {
                open_url(&u.page_url);
            }
        }
        _ => {}
    }
}

pub fn tick() {
    let finished = UPDATER.with(|u| {
        let mut u = u.borrow_mut();
        let done = u.checking.as_ref().and_then(|(a, manual)| a.lock().unwrap_or_else(|e| e.into_inner()).take().map(|r| (r, *manual)));
        if done.is_some() {
            u.checking = None;
        }
        done
    });
    if let Some((result, manual)) = finished {
        match result {
            Ok(Some(info)) => {
                crate::log(&format!("Update available: {} (running {})", info.version, version()));
                UPDATER.with(|u| u.borrow_mut().available = Some(info));
                super::full::update_changed();
                if manual {
                    ask_to_install();
                }
            }
            Ok(None) if manual => message_box("You're up to date", &format!("WebSkin Player {} is the newest version.", version())),
            Err(e) if manual => message_box("Couldn't check for updates", &e),
            Err(e) => crate::log(&format!("Update check failed: {e}")),
            Ok(None) => {}
        }
    }
    let progress = UPDATER.with(|u| u.borrow().download.as_ref().map(|(i, d)| (i.clone(), d.progress())));
    if let Some((info, p)) = progress {
        super::full::update_changed();
        if let Progress::Done(result) = p {
            UPDATER.with(|u| u.borrow_mut().download = None);
            match result.and_then(|zip| install(&info, &zip)) {
                Ok(()) => super::push_command("quit"),
                Err(e) => {
                    crate::log(&format!("Update failed: {e}"));
                    if e != "cancelled" {
                        message_box("The update couldn't be installed", &format!("{e}\n\nYou can download it from the release page instead."));
                        open_url(&info.page_url);
                    }
                }
            }
        }
    }
}

/// The running app bundle, when it is one this user can replace.
fn app_bundle() -> Option<PathBuf> {
    let path = PathBuf::from(NSBundle::mainBundle().bundlePath().to_string());
    if path.extension().is_none_or(|e| e != "app") {
        return None;
    }
    let parent = path.parent()?;
    let c = std::ffi::CString::new(parent.as_os_str().as_encoded_bytes()).ok()?;
    (unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0).then_some(path)
}

fn work_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("WebSkin Player").join("Update")
}

fn ask_to_install() {
    let Some(info) = available() else { return };
    if status().is_some() {
        return;
    }
    let alert = NSAlert::new(mtm());
    alert.setMessageText(&NSString::from_str(&format!("WebSkin Player {} is available", info.version)));
    let can_install = info.zip_url.is_some() && app_bundle().is_some();
    alert.setInformativeText(&NSString::from_str(&if can_install {
        format!(
            "You have {}. Install Now downloads the new version, checks it, and restarts the player with it; your skins and settings stay.",
            version()
        )
    } else {
        format!("You have {}. Download the new version from its release page.", version())
    }));
    alert.addButtonWithTitle(&NSString::from_str(if can_install { "Install Now" } else { "Open Release Page" }));
    alert.addButtonWithTitle(&NSString::from_str("What's New"));
    alert.addButtonWithTitle(&NSString::from_str("Later"));
    NSApplication::sharedApplication(mtm()).activate();
    let answer = alert.runModal();
    if answer == NSAlertFirstButtonReturn {
        match info.zip_url.clone().filter(|_| can_install) {
            Some(url) => {
                let dest = work_dir().join(format!("WebSkinPlayer-{}-mac.zip", info.version));
                crate::log(&format!("Downloading update {}", info.version));
                let d = Download::start(&url, &dest, info.size, crate::net::check_zip);
                UPDATER.with(|u| u.borrow_mut().download = Some((info, d)));
                super::full::update_changed();
            }
            None => open_url(&info.page_url),
        }
    } else if answer == NSAlertSecondButtonReturn {
        open_url(&info.page_url);
    }
}

/// Checks the download, unpacks it, and sets a helper to swap the app in once this copy has quit.
fn install(info: &UpdateInfo, zip: &Path) -> Result<(), String> {
    let length = std::fs::metadata(zip).map_err(|e| e.to_string())?.len();
    if info.size > 0 && length != info.size {
        return Err(format!("The download is {length} bytes; the release says {}.", info.size));
    }
    if let Some(expected) = &info.sha256 {
        let actual = crate::net::sha256(zip)?;
        if &actual != expected {
            return Err("The download doesn't match the release's checksum.".into());
        }
    }
    let target = app_bundle().ok_or("This copy of the app can't be replaced from here.")?;
    let unpacked = work_dir().join("unpacked");
    let _ = std::fs::remove_dir_all(&unpacked);
    let status = std::process::Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(zip).arg(&unpacked).status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("The download couldn't be unpacked.".into());
    }
    let new_app = unpacked.join("WebSkin Player.app");
    if !new_app.join("Contents/MacOS/webskin").exists() {
        return Err("The download doesn't hold WebSkin Player.app.".into());
    }
    // Waits for this copy to quit, swaps the new app in (keeping the old one until that worked), starts it.
    // The new app is copied next to the old one first, so the swap is two renames on one disk and the
    // installed app is never left half-replaced.
    let script = r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
new="$2.new"
old="$2.previous"
rm -rf "$new" "$old"
if ditto "$3" "$new"; then
  if mv "$2" "$old"; then
    if mv "$new" "$2"; then rm -rf "$old"; else mv "$old" "$2"; fi
  fi
fi
rm -rf "$new"
open "$2""#;
    std::process::Command::new("/bin/sh")
        .args(["-c", script, "webskin-update", &std::process::id().to_string()])
        .arg(&target)
        .arg(&new_app)
        .spawn()
        .map_err(|e| e.to_string())?;
    crate::log(&format!("Update {} verified; restarting into it", info.version));
    Ok(())
}
