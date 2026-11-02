//! `webskin --snapshot <out> [skin]`: a developer hook that walks through the app's windows on its own
//! (full mode's two tabs, the menu-bar player, the screen saver, and a skin with Always on Top) and saves
//! a PNG of each, plus `snapshot.txt` describing what it found. CI runs it on a Mac so the windows can be
//! looked at without a Mac at hand.

use super::{full, run_command, saver, widget};
use objc2::AllocAnyThread;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSView, NSWindow};
use objc2_core_graphics::{CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_foundation::NSDictionary;
use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct Run {
    out: PathBuf,
    skin: Option<PathBuf>,
    /// When the current step became due to start counting its delay.
    since: Option<Instant>,
    step: usize,
    report: String,
}

thread_local! {
    static RUN: RefCell<Option<Run>> = const { RefCell::new(None) };
}

/// A snapshot run is under way (dialogs go to the log instead).
pub fn active() -> bool {
    RUN.with(|r| r.borrow().is_some())
}

/// Called from main before the app runs.
pub fn configure(out: PathBuf, skin: Option<PathBuf>) {
    let _ = std::fs::create_dir_all(&out);
    RUN.with(|r| *r.borrow_mut() = Some(Run { out, skin, since: None, step: 0, report: String::new() }));
}

pub fn begin() {
    RUN.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.since = Some(Instant::now());
        }
    });
}

fn note(line: &str) {
    crate::log(&format!("snapshot: {line}"));
    RUN.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            let _ = writeln!(r.report, "{line}");
        }
    });
}

fn out_dir() -> Option<PathBuf> {
    RUN.with(|r| r.borrow().as_ref().map(|r| r.out.clone()))
}

/// Draws a view's layer tree (which holds what our views show) into a PNG.
fn save_view(view: &NSView, name: &str) {
    let Some(dir) = out_dir() else { return };
    let path = dir.join(format!("{name}.png"));
    let scale = view.window().map_or(2.0, |w| w.backingScaleFactor());
    let b = view.bounds();
    let (w, h) = ((b.size.width * scale).round() as usize, (b.size.height * scale).round() as usize);
    let saved = (|| {
        let layer = view.layer()?;
        let space = CGColorSpace::new_device_rgb()?;
        let info = CGImageByteOrderInfo::Order32Big.0 | CGImageAlphaInfo::PremultipliedLast.0;
        let ctx = unsafe { CGBitmapContextCreate(std::ptr::null_mut(), w, h, 8, w * 4, Some(&space), info) }?;
        if view.isFlipped() {
            CGContext::translate_ctm(Some(&ctx), 0.0, h as f64);
            CGContext::scale_ctm(Some(&ctx), scale, -scale);
        } else {
            CGContext::scale_ctm(Some(&ctx), scale, scale);
        }
        layer.renderInContext(&ctx);
        let image = CGBitmapContextCreateImage(Some(&ctx))?;
        write_png(&image, &path)
    })();
    // Views without a layer: AppKit's own drawing.
    let saved = saved.or_else(|| {
        let rep = view.bitmapImageRepForCachingDisplayInRect(b)?;
        view.cacheDisplayInRect_toBitmapImageRep(b, &rep);
        let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
        std::fs::write(&path, data.to_vec()).ok()
    });
    note(&format!("{name}: {}", if saved.is_some() { format!("saved {}x{}", w, h) } else { "could not be saved".to_string() }));
}

fn write_png(image: &objc2_core_graphics::CGImage, path: &Path) -> Option<()> {
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), image);
    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
    std::fs::write(path, data.to_vec()).ok()
}

fn save_window(window: &NSWindow, name: &str) {
    if let Some(view) = window.contentView() {
        save_view(&view, name);
    }
    note(&format!("{name}: window level {}, visible {}", window.level(), window.isVisible()));
}

/// A step runs this long after the previous one finished; it returns false to be tried again next tick
/// (waiting for something), giving up after a minute and a half.
type Step = (f64, fn() -> bool);

fn steps() -> Vec<Step> {
    vec![
        (0.5, || {
            run_command("fullmode");
            true
        }),
        (5.0, || {
            if let Some(w) = full::window() {
                save_window(&w, "full-now-playing");
            }
            note(&format!("update offered: {:?}", super::updater::available().map(|u| u.version)));
            true
        }),
        (0.2, || {
            full::select(full::Tab::Skins);
            true
        }),
        (5.0, || {
            if let Some(w) = full::window() {
                save_window(&w, "full-skin-chooser");
            }
            note(&format!("chooser preview shown: {}", super::chooser::preview_ready()));
            true
        }),
        (0.2, || {
            full::select(full::Tab::OnlineSkins);
            true
        }),
        (1.0, super::chooser::online_loaded),
        (8.0, || {
            if let Some(w) = full::window() {
                save_window(&w, "full-get-more-skins");
            }
            note(&format!("archive.org list loaded: {}, preview shown: {}", super::chooser::online_loaded(), super::chooser::preview_ready()));
            note(&format!("downloading and applying: {:?}", super::chooser::download_first()));
            true
        }),
        (1.0, || super::mode() == super::Mode::Skin),
        (4.0, || {
            note(&format!("after the download: mode {:?}, skin windows {}", super::mode(), super::skin_window_list().len()));
            for (v, w) in super::skin_window_list() {
                save_window(&w, &format!("downloaded-skin-view-{v}"));
            }
            run_command("fullmode");
            true
        }),
        (0.5, || {
            run_command("ontop");
            true
        }),
        (0.5, || {
            if let Some(w) = full::window() {
                note(&format!("always on top: full mode level {}", w.level()));
            }
            run_command("ontop");
            true
        }),
        (0.5, || {
            run_command("menubarmode");
            true
        }),
        (1.0, || {
            note(&format!("menu-bar player shown: {}, full mode shown: {}", widget::is_shown(), full::is_shown()));
            if let Some(v) = widget::header_view() {
                save_view(&v, "menubar-player-menu");
            }
            if let Some(img) = widget::button_image() {
                let _ = out_dir().map(|d| std::fs::write(d.join("menubar-item.tiff"), img));
            }
            saver::start(true);
            true
        }),
        (4.0, || {
            if let Some(w) = saver::first_window() {
                save_window(&w, "screen-saver");
            }
            saver::stop(false);
            let skin = RUN.with(|r| r.borrow().as_ref().and_then(|r| r.skin.clone()));
            if let Some(path) = skin {
                super::open_skin_now(&path);
            }
            run_command("ontop");
            true
        }),
        (4.0, || {
            for (v, w) in super::skin_window_list() {
                save_window(&w, &format!("skin-view-{v}"));
            }
            note(&format!("mode {:?}, global visualization {:?}", super::mode(), super::global_vis()));
            run_command("ontop");
            true
        }),
        (0.5, || {
            finish();
            true
        }),
    ]
}

fn finish() {
    if let Some(r) = RUN.with(|r| r.borrow_mut().take()) {
        let _ = std::fs::write(r.out.join("snapshot.txt"), r.report);
    }
    run_command("quit");
}

pub fn tick() {
    let due = RUN.with(|r| {
        let r = r.borrow();
        let r = r.as_ref()?;
        let waited = r.since?.elapsed().as_secs_f64();
        let (delay, f) = *steps().get(r.step)?;
        (waited >= delay).then_some((f, waited >= delay + 90.0))
    });
    let Some((step, overdue)) = due else { return };
    let done = step();
    if done || overdue {
        if !done {
            note(&format!("step {} gave up waiting", RUN.with(|r| r.borrow().as_ref().map_or(0, |r| r.step))));
        }
        RUN.with(|r| {
            if let Some(r) = r.borrow_mut().as_mut() {
                r.step += 1;
                r.since = Some(Instant::now());
            }
        });
    }
}
