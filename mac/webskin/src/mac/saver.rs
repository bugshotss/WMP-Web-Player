//! The visualizer screen saver: once the Mac has been idle for a while, every display shows a random
//! visualization (changing every so often) with the current track fading in, until the mouse or keyboard
//! is used. It can lock the Mac as it closes. It stays away while a video, presentation or game keeps the
//! display awake (music playing in a browser tab doesn't), and while the Mac is locked.

use super::ui::{self, Pin, rect};
use super::{mtm, settings, update_settings};
use crate::skin::{AppAction, MenuItem};
use crate::vis::{ALL, Renderer, VisFrame};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSColor, NSCursor, NSEvent, NSResponder, NSScreen, NSScreenSaverWindowLevel, NSTextField, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
use objc2_foundation::{NSDictionary, NSNumber, NSObject, NSString};
use objc2_quartz_core::kCAGravityResize;
use std::cell::RefCell;
use std::ffi::{CString, c_void};
use std::time::{Duration, Instant};

define_class!(
    /// A screen saver window: it takes the keyboard, so the key that closes the screen saver doesn't reach
    /// a skin or full mode underneath.
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinSaverWindow"]
    struct SaverPanel;

    impl SaverPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, _event: &NSEvent) {}
    }
);

pub const IDLE_CHOICES: [u32; 8] = [1, 2, 3, 5, 10, 15, 20, 30];
pub const CHANGE_CHOICES: [u32; 5] = [15, 30, 60, 120, 300];

/// One display's window: the shared picture stretched over it, the names, and a black layer for fades.
struct SaverWindow {
    window: Retained<NSWindow>,
    picture: Retained<NSView>,
    info: Retained<NSView>,
    title: Retained<NSTextField>,
    artist: Retained<NSTextField>,
    vis_name: Retained<NSTextField>,
    fade: Retained<NSView>,
}

struct Running {
    windows: Vec<SaverWindow>,
    renderer: Renderer,
    current: (usize, usize),
    render_size: (usize, usize),
    started: Instant,
    rotated: Instant,
    /// When the names last faded in, and how long they stay.
    info_since: Instant,
    info_hold: f64,
    /// A change of visualization under way: when it started, and what comes next.
    fading: Option<(Instant, (usize, usize))>,
    track_version: u32,
    mouse_at: (f64, f64),
    /// Input before this moment doesn't count (the click or key that started it).
    baseline: Instant,
}

#[derive(Default)]
struct Saver {
    running: Option<Running>,
    last_check: Option<Instant>,
    seed: u64,
}

thread_local! {
    static SAVER: RefCell<Saver> = RefCell::default();
}

pub fn is_running() -> bool {
    SAVER.with(|s| s.borrow().running.is_some())
}

/// The "Screen Saver" submenu, for the menu bar and every right-click menu.
pub fn menu_entry() -> (MenuItem, AppAction) {
    let (enabled, idle, change, locks) = settings(|s| (s.saver_enabled, s.saver_idle(), s.saver_change(), s.saver_locks));
    let host = |c: String| AppAction::Host(c);
    let (after_items, after_actions): (Vec<_>, Vec<_>) = IDLE_CHOICES
        .iter()
        .map(|&m| {
            (MenuItem::item(&if m == 1 { "1 minute".to_string() } else { format!("{m} minutes") }, true, idle == m), host(format!("saver:idle:{m}")))
        })
        .unzip();
    let (change_items, change_actions): (Vec<_>, Vec<_>) = CHANGE_CHOICES
        .iter()
        .map(|&sec| {
            let label = match sec {
                s if s < 60 => format!("{s} seconds"),
                60 => "1 minute".into(),
                s => format!("{} minutes", s / 60),
            };
            (MenuItem::item(&label, true, change == sec), host(format!("saver:change:{sec}")))
        })
        .unzip();
    let items = vec![
        (MenuItem::item("Use the Visualizer Screen Saver", true, enabled), host("saver:toggle".into())),
        (MenuItem::submenu("Start After", after_items), AppAction::Submenu(after_actions)),
        (MenuItem::submenu("Change Visualization Every", change_items), AppAction::Submenu(change_actions)),
        (MenuItem::item("Lock the Mac When It Closes", true, locks), host("saver:lock".into())),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Start Now", true, false), host("saver:start".into())),
    ];
    let (items, actions): (Vec<_>, Vec<_>) = items.into_iter().unzip();
    (MenuItem { checked: enabled, ..MenuItem::submenu("Screen Saver", items) }, AppAction::Submenu(actions))
}

pub fn command(c: &str) {
    let parts: Vec<&str> = c.split(':').collect();
    match parts.as_slice() {
        ["saver", "toggle"] => update_settings(|s| s.saver_enabled = !s.saver_enabled),
        ["saver", "lock"] => update_settings(|s| s.saver_locks = !s.saver_locks),
        ["saver", "start"] => start(true),
        ["saver", "stop"] => stop(false),
        ["saver", "next"] => next(),
        ["saver", "idle", m] => {
            if let Ok(m) = m.parse() {
                update_settings(|s| s.saver_idle_minutes = m);
            }
        }
        ["saver", "change", sec] => {
            if let Ok(sec) = sec.parse() {
                update_settings(|s| s.saver_change_seconds = sec);
            }
        }
        _ => {}
    }
}

// ---- when to start ---------------------------------------------------------------------------

/// Seconds since the last input of a kind (or any, with `None`).
fn idle_seconds(kind: Option<CGEventType>) -> f64 {
    CGEventSource::seconds_since_last_event_type(CGEventSourceStateID::CombinedSessionState, kind.unwrap_or(CGEventType(u32::MAX)))
}

/// The screen is locked (or the login window is up).
fn screen_locked() -> bool {
    let Some(dict) = objc2_core_graphics::CGSessionCopyCurrentDictionary() else { return false };
    let d: &NSDictionary<NSString, AnyObject> = unsafe { &*objc2_core_foundation::CFRetained::as_ptr(&dict).as_ptr().cast() };
    d.objectForKey(&NSString::from_str("CGSSessionScreenIsLocked")).and_then(|v| v.downcast::<NSNumber>().ok()).is_some_and(|n| n.boolValue())
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMCopyAssertionsStatus(status: *mut *const c_void) -> i32;
}

/// Something keeps the display awake: a video playing, a presentation, a game (as Windows' own screen saver
/// stays away for them). Music alone keeps the system, not the display, awake.
fn something_needs_the_screen() -> bool {
    let mut status: *const c_void = std::ptr::null();
    if unsafe { IOPMCopyAssertionsStatus(&mut status) } != 0 || status.is_null() {
        return false;
    }
    let d: Retained<NSDictionary<NSString, AnyObject>> = unsafe { Retained::from_raw(status as *mut _) }.expect("non-null");
    ["PreventUserIdleDisplaySleep", "NoDisplaySleepAssertion"]
        .iter()
        .any(|k| d.objectForKey(&NSString::from_str(k)).and_then(|v| v.downcast::<NSNumber>().ok()).is_some_and(|n| n.intValue() > 0))
}

fn check_idle() {
    let due = SAVER.with(|s| s.borrow().last_check.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)));
    if !due {
        return;
    }
    SAVER.with(|s| s.borrow_mut().last_check = Some(Instant::now()));
    let (enabled, idle) = settings(|s| (s.saver_enabled, s.saver_idle()));
    if !enabled || is_running() || std::env::var_os("WEBSKIN_HOME").is_some() && std::env::var_os("WEBSKIN_SAVER").is_none() {
        return;
    }
    if idle_seconds(None) < f64::from(idle) * 60.0 || screen_locked() || something_needs_the_screen() {
        return;
    }
    start(false);
}

// ---- running ---------------------------------------------------------------------------------

/// Every visualization and preset worth watching full screen (album art isn't one).
fn choices() -> Vec<(usize, usize)> {
    ALL.iter().enumerate().filter(|(_, d)| d.id != "AlbumArt").flat_map(|(i, d)| (0..d.presets.len().max(1)).map(move |p| (i, p))).collect()
}

fn random_choice(except: (usize, usize)) -> (usize, usize) {
    let list: Vec<_> = choices().into_iter().filter(|c| *c != except).collect();
    let n = SAVER.with(|s| {
        let mut s = s.borrow_mut();
        if s.seed == 0 {
            s.seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
        }
        // xorshift64
        s.seed ^= s.seed << 13;
        s.seed ^= s.seed >> 7;
        s.seed ^= s.seed << 17;
        s.seed
    });
    list.get((n % list.len().max(1) as u64) as usize).copied().unwrap_or((0, 0))
}

fn name_of((effect, preset): (usize, usize)) -> String {
    let d = &ALL[effect];
    match d.presets.get(preset) {
        Some(p) if *p != d.title => format!("{}: {p}", d.title),
        _ => d.title.to_string(),
    }
}

fn saver_window(mtm: MainThreadMarker, screen: &NSScreen) -> SaverWindow {
    let frame = screen.frame();
    let panel: Retained<SaverPanel> = unsafe {
        msg_send![SaverPanel::alloc(mtm), initWithContentRect: frame, styleMask: NSWindowStyleMask::Borderless, backing: NSBackingStoreType::Buffered, defer: false]
    };
    let window: Retained<NSWindow> = Retained::into_super(panel);
    unsafe { window.setReleasedWhenClosed(false) };
    window.setLevel(NSScreenSaverWindowLevel);
    window.setBackgroundColor(Some(&NSColor::blackColor()));
    window.setOpaque(true);
    window.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary);
    window.setTitle(&NSString::from_str("WebSkin Player Screen Saver"));
    let (w, h) = (frame.size.width, frame.size.height);
    let root = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, w, h));
    root.setWantsLayer(true);
    window.setContentView(Some(&root));
    let picture = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, w, h));
    picture.setWantsLayer(true);
    if let Some(layer) = picture.layer() {
        layer.setContentsGravity(unsafe { kCAGravityResize });
    }
    ui::place(&picture, &root, rect(0.0, 0.0, w, h), Pin::FILL);
    // The names sit at the bottom left (Cocoa's y runs up).
    let info = NSView::initWithFrame(NSView::alloc(mtm), rect(56.0, 48.0, w - 112.0, 110.0));
    info.setWantsLayer(true);
    let title = ui::label(mtm, "", 30.0, true, 0xFFFFFF);
    let artist = ui::label(mtm, "", 20.0, false, 0xB9C4D2);
    let vis_name = ui::label(mtm, "", 13.0, false, 0x8B95A3);
    let shadow = objc2_app_kit::NSShadow::new();
    shadow.setShadowBlurRadius(8.0);
    shadow.setShadowColor(Some(&NSColor::blackColor()));
    for (l, y, hh) in [(&title, 66.0, 40.0), (&artist, 38.0, 28.0), (&vis_name, 0.0, 20.0)] {
        l.setShadow(Some(&shadow));
        ui::place(l, &info, rect(0.0, y, w - 112.0, hh), Pin::WIDTH);
    }
    info.setAlphaValue(0.0);
    root.addSubview(&info);
    let fade = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, w, h));
    fade.setWantsLayer(true);
    if let Some(layer) = fade.layer() {
        layer.setBackgroundColor(Some(&objc2_core_graphics::CGColor::new_srgb(0.0, 0.0, 0.0, 1.0)));
    }
    fade.setAlphaValue(0.0);
    ui::place(&fade, &root, rect(0.0, 0.0, w, h), Pin::FILL);
    SaverWindow { window, picture, info, title, artist, vis_name, fade }
}

pub fn start(manual: bool) {
    if is_running() {
        return;
    }
    let mtm = mtm();
    let screens = NSScreen::screens(mtm);
    if screens.is_empty() {
        return;
    }
    let idle = settings(|s| s.saver_idle());
    crate::log(&format!("Screen saver starting ({})", if manual { "manually".to_string() } else { format!("idle {idle} min") }));
    // Rendered once at the main screen's shape (capped by the renderer) and stretched onto every screen.
    let main = screens.firstObject().map_or((1280.0, 720.0), |s| (s.frame().size.width, s.frame().size.height));
    let current = random_choice((usize::MAX, 0));
    let mut renderer = Renderer::new(current.0, current.1);
    renderer.max_pixels = 1280 * 720;
    let windows: Vec<SaverWindow> = screens.iter().map(|s| saver_window(mtm, &s)).collect();
    for w in &windows {
        w.window.orderFrontRegardless();
    }
    if let Some(first) = windows.first() {
        NSApplication::sharedApplication(mtm).activate();
        first.window.makeKeyAndOrderFront(None);
    }
    NSCursor::hide();
    let now = Instant::now();
    let p = NSEvent::mouseLocation();
    SAVER.with(|s| {
        s.borrow_mut().running = Some(Running {
            windows,
            renderer,
            current,
            render_size: ((main.0 * 2.0) as usize, (main.1 * 2.0) as usize),
            started: now,
            rotated: now,
            info_since: now,
            info_hold: 10.0,
            fading: None,
            track_version: u32::MAX,
            mouse_at: (p.x, p.y),
            baseline: now,
        })
    });
    show_names();
}

pub fn stop(lock: bool) {
    let Some(r) = SAVER.with(|s| s.borrow_mut().running.take()) else { return };
    for w in &r.windows {
        w.window.orderOut(None);
        w.window.close();
    }
    NSCursor::unhide();
    crate::log(&format!("Screen saver closed{}", if lock { ", locking the Mac" } else { "" }));
    if lock {
        lock_screen();
    }
}

fn next() {
    SAVER.with(|s| {
        if let Some(r) = s.borrow_mut().running.as_mut() {
            r.rotated = Instant::now() - Duration::from_secs(3600);
        }
    });
}

/// Locks the Mac as the lock-screen shortcut does.
fn lock_screen() {
    unsafe {
        if let Ok(path) = CString::new("/System/Library/PrivateFrameworks/login.framework/Versions/Current/login") {
            let h = libc::dlopen(path.as_ptr(), libc::RTLD_NOW);
            if !h.is_null()
                && let Ok(name) = CString::new("SACLockScreenImmediate")
            {
                let f = libc::dlsym(h, name.as_ptr());
                if !f.is_null() {
                    let lock: unsafe extern "C" fn() -> i32 = std::mem::transmute(f);
                    lock();
                    return;
                }
            }
        }
    }
    // Sleeping the display locks it when the Mac asks for the password right away.
    let _ = std::process::Command::new("/usr/bin/pmset").arg("displaysleepnow").spawn();
}

/// Shows the track and visualization names, fading them in and out.
fn show_names() {
    let n = super::now();
    SAVER.with(|s| {
        let mut s = s.borrow_mut();
        let Some(r) = s.running.as_mut() else { return };
        let (title, artist) = if n.media.has_session() { (n.media.title.clone(), n.media.artist.clone()) } else { (String::new(), String::new()) };
        let vis = name_of(r.current);
        for w in &r.windows {
            ui::set_text(&w.title, &title);
            ui::set_text(&w.artist, &artist);
            ui::set_text(&w.vis_name, &vis);
        }
        r.info_since = Instant::now();
        r.info_hold = if title.is_empty() { 6.0 } else { 10.0 };
        r.track_version = n.version;
    });
}

pub fn render(frame: &VisFrame) {
    let image = SAVER.with(|s| {
        let mut s = s.borrow_mut();
        let r = s.running.as_mut()?;
        let colors = crate::vis::colors::map_for(&super::vis_colors(r.current.0));
        let ctx = super::vis_context();
        let (w, h) = r.render_size;
        r.renderer.render(frame, &ctx, w, h, colors.as_deref());
        r.renderer.image.as_ref().and_then(|pm| ui::cg_image(pm, None))
    });
    let Some(image) = image else { return };
    SAVER.with(|s| {
        if let Some(r) = s.borrow().running.as_ref() {
            for w in &r.windows {
                if let Some(layer) = w.picture.layer() {
                    unsafe { layer.setContents(Some(ui::layer_contents(&image))) };
                }
            }
        }
    });
}

pub fn tick() {
    if !is_running() {
        check_idle();
        return;
    }
    // Any key, click or scroll closes it; so does moving the mouse more than a few points.
    let since_start = SAVER.with(|s| s.borrow().running.as_ref().map(|r| (r.baseline.elapsed().as_secs_f64(), r.started.elapsed(), r.mouse_at)));
    let Some((since, running_for, mouse_at)) = since_start else { return };
    let acted =
        [CGEventType::KeyDown, CGEventType::LeftMouseDown, CGEventType::RightMouseDown, CGEventType::OtherMouseDown, CGEventType::ScrollWheel]
            .into_iter()
            .any(|k| idle_seconds(Some(k)) < since);
    let p = NSEvent::mouseLocation();
    let moved = (p.x - mouse_at.0).abs() + (p.y - mouse_at.1).abs();
    // A second and a half of grace, so the click or key that started it doesn't close it again.
    if running_for < Duration::from_millis(1500) {
        SAVER.with(|s| {
            if let Some(r) = s.borrow_mut().running.as_mut() {
                r.baseline = Instant::now();
                r.mouse_at = (p.x, p.y);
            }
        });
    } else if acted || moved > 12.0 {
        stop(settings(|s| s.saver_locks));
        return;
    }
    let change = Duration::from_secs(u64::from(settings(|s| s.saver_change())));
    let track = super::now().version;
    let mut names = false;
    SAVER.with(|s| {
        let mut s = s.borrow_mut();
        let Some(r) = s.running.as_mut() else { return };
        if r.track_version != track {
            names = true;
        }
        // The names: in over 0.6 s, held, out over 1.2 s.
        let t = r.info_since.elapsed().as_secs_f64();
        let alpha = if t < 0.6 {
            t / 0.6
        } else if t < r.info_hold {
            1.0
        } else {
            (1.0 - (t - r.info_hold) / 1.2).max(0.0)
        };
        // A change of visualization: to black over 0.4 s, swap, back over 0.5 s.
        let mut fade = 0.0;
        if let Some((since, next)) = r.fading {
            let f = since.elapsed().as_secs_f64();
            if f < 0.4 {
                fade = f / 0.4;
            } else {
                if r.current != next {
                    r.current = next;
                    r.renderer.set(next.0, next.1);
                    names = true;
                }
                fade = (1.0 - (f - 0.4) / 0.5).max(0.0);
                if f >= 0.9 {
                    r.fading = None;
                    r.rotated = Instant::now();
                }
            }
        } else if r.rotated.elapsed() >= change {
            r.fading = Some((Instant::now(), (usize::MAX, 0)));
        }
        for w in &r.windows {
            w.info.setAlphaValue(alpha);
            w.fade.setAlphaValue(fade);
        }
    });
    // Pick the next visualization outside the borrow (it draws a random number).
    let needs_pick = SAVER.with(|s| s.borrow().running.as_ref().is_some_and(|r| r.fading.is_some_and(|(_, n)| n.0 == usize::MAX)));
    if needs_pick {
        let current = SAVER.with(|s| s.borrow().running.as_ref().map(|r| r.current)).unwrap_or((0, 0));
        let next = random_choice(current);
        SAVER.with(|s| {
            if let Some(r) = s.borrow_mut().running.as_mut()
                && let Some((since, _)) = r.fading
            {
                r.fading = Some((since, next));
            }
        });
    }
    if names {
        show_names();
    }
}

/// For the snapshot hook: the first display's window while it runs.
pub fn first_window() -> Option<Retained<NSWindow>> {
    SAVER.with(|s| s.borrow().running.as_ref().and_then(|r| r.windows.first().map(|w| w.window.clone())))
}
