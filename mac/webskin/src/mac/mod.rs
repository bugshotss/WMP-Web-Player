//! The macOS app: one window per open skin view, a menu bar, and the Now Playing session.

mod nowplaying;
mod volume;
mod window;

use crate::render::Rect;
use crate::settings::Settings;
use crate::skin::{Host, MenuItem, Shared, ViewId};
use block2::RcBlock;
use nowplaying::{NowPlaying, Source};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSMenuItem,
    NSOpenPanel, NSScreen, NSSound, NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSData, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer, NSURL,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;
use window::ViewWindow;

enum Pending {
    ChooseSkin,
    Load(PathBuf),
}

#[derive(Default)]
struct AppState {
    shared: Option<Rc<Shared>>,
    windows: HashMap<ViewId, Rc<ViewWindow>>,
    media: Option<Rc<NowPlaying>>,
    pending: Vec<Pending>,
    /// A new skin is replacing the open one: its closing doesn't quit.
    replacing: bool,
    first_skin: Option<PathBuf>,
}

thread_local! {
    static APP: RefCell<AppState> = RefCell::default();
    static CHOSEN: Cell<Option<isize>> = const { Cell::new(None) };
}

/// The open skin.
pub(crate) fn current() -> Option<Rc<Shared>> {
    APP.with(|a| a.borrow().shared.clone())
}

fn window_for(v: ViewId) -> Option<Rc<ViewWindow>> {
    APP.with(|a| a.borrow().windows.get(&v).cloned())
}

fn push(p: Pending) {
    APP.with(|a| a.borrow_mut().pending.push(p));
}

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("the app runs on the main thread")
}

pub(crate) fn quit() {
    let app = NSApplication::sharedApplication(mtm());
    app.terminate(None);
}

/// Renders a view into its window.
pub(crate) fn redraw(v: ViewId) {
    let (Some(shared), Some(w)) = (current(), window_for(v)) else { return };
    let scale = w.scale();
    let canvas = shared.read(|s| s.views.get(v).and_then(Option::as_ref).and_then(|_| s.render_view(v, scale)));
    if let Some(c) = canvas {
        w.present(&c);
    }
}

struct MacHost;

impl Host for MacHost {
    fn open_url(&self, url: &str) {
        if let Some(u) = NSURL::URLWithString(&NSString::from_str(url)) {
            NSWorkspace::sharedWorkspace().openURL(&u);
        }
    }

    fn view_opened(&self, v: ViewId) {
        let Some(shared) = current() else { return };
        let (title, left, top, w, h) = shared.read(|s| (s.get_str(v, 0, "title"), s.view(v).left, s.view(v).top, s.width(v, 0), s.height(v, 0)));
        let window = Rc::new(ViewWindow::open(mtm(), v, &title, left, top, f64::from(w), f64::from(h)));
        APP.with(|a| a.borrow_mut().windows.insert(v, window));
        redraw(v);
    }

    fn view_closed(&self, v: ViewId) {
        if let Some(w) = APP.with(|a| a.borrow_mut().windows.remove(&v)) {
            w.close();
        }
    }

    fn view_changed(&self, v: ViewId) {
        let (Some(shared), Some(w)) = (current(), window_for(v)) else { return };
        let Some((title, width, height)) =
            shared.read(|s| s.views.get(v).and_then(Option::as_ref).map(|_| (s.get_str(v, 0, "title"), s.width(v, 0), s.height(v, 0))))
        else {
            return;
        };
        w.window.setTitle(&NSString::from_str(&title));
        w.resize(f64::from(width), f64::from(height));
        redraw(v);
    }

    fn view_moved(&self, v: ViewId) {
        let (Some(shared), Some(w)) = (current(), window_for(v)) else { return };
        let (left, top) = shared.read(|s| (s.view(v).left, s.view(v).top));
        w.place(left, top);
    }

    fn minimize(&self, v: ViewId) {
        if let Some(w) = window_for(v) {
            w.minimize();
        }
    }

    fn begin_move(&self, v: ViewId) {
        if let Some(w) = window_for(v) {
            w.begin_move();
        }
    }

    fn begin_resize(&self, v: ViewId, edge: &str) {
        if let Some(w) = window_for(v) {
            w.begin_resize(v, edge);
        }
    }

    fn redraw(&self, v: ViewId) {
        redraw(v);
    }

    fn set_tooltip(&self, v: ViewId, text: Option<&str>) {
        if let Some(w) = window_for(v) {
            w.set_tooltip(text);
        }
    }

    fn set_cursor(&self, v: ViewId, cursor: &str) {
        if let Some(w) = window_for(v) {
            w.set_cursor(cursor);
        }
    }

    fn show_menu(&self, v: ViewId, items: &[MenuItem], at: (f64, f64)) -> Option<Vec<usize>> {
        let w = window_for(v)?;
        let target = MenuTarget::new(mtm());
        let mut paths = vec![];
        let menu = build_menu(items, &[], &mut paths, &target);
        CHOSEN.with(|c| c.set(None));
        let view = w.window.contentView()?;
        menu.popUpMenuPositioningItem_atLocation_inView(None, NSPoint::new(at.0, at.1), Some(&view));
        let tag = CHOSEN.with(Cell::take)?;
        paths.get(usize::try_from(tag).ok()?).cloned()
    }

    fn message_box(&self, title: &str, text: &str) {
        let alert = NSAlert::new(mtm());
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(text));
        alert.runModal();
    }

    fn play_sound(&self, bytes: Vec<u8>) {
        let data = NSData::with_bytes(&bytes);
        if let Some(sound) = NSSound::initWithData(NSSound::alloc(), &data) {
            sound.play();
        }
    }

    fn work_area(&self) -> Rect {
        work_area()
    }

    fn screen_size(&self) -> (f64, f64) {
        NSScreen::mainScreen(mtm()).map_or((1440.0, 900.0), |s| (s.frame().size.width, s.frame().size.height))
    }

    fn skin_closed(&self) {
        let replacing = APP.with(|a| a.borrow().replacing);
        if !replacing {
            quit();
        }
    }

    fn choose_skin(&self) {
        push(Pending::ChooseSkin);
    }
}

/// The visible desktop of the main screen, top-left origin.
fn work_area() -> Rect {
    let mtm = mtm();
    let primary = NSScreen::screens(mtm).firstObject().map_or(900.0, |s| s.frame().size.height);
    NSScreen::mainScreen(mtm).map_or(Rect::new(0.0, 0.0, 1440.0, 900.0), |s| {
        let f = s.visibleFrame();
        Rect::new(f.origin.x, primary - f.origin.y - f.size.height, f.size.width, f.size.height)
    })
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(choose:))]
        fn choose(&self, sender: &NSMenuItem) {
            CHOSEN.with(|c| c.set(Some(sender.tag())));
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

fn build_menu(items: &[MenuItem], prefix: &[usize], paths: &mut Vec<Vec<usize>>, target: &MenuTarget) -> Retained<NSMenu> {
    let mtm = mtm();
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    for (i, item) in items.iter().enumerate() {
        if item.separator {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            continue;
        }
        let path: Vec<usize> = prefix.iter().copied().chain([i]).collect();
        let action = item.submenu.is_empty().then_some(sel!(choose:));
        let mi = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(&item.label), action, &NSString::new())
        };
        mi.setEnabled(item.enabled);
        mi.setState(if item.checked { NSControlStateValueOn } else { NSControlStateValueOff });
        if item.submenu.is_empty() {
            unsafe { mi.setTarget(Some(target)) };
            mi.setTag(paths.len() as isize);
            paths.push(path);
        } else {
            mi.setSubmenu(Some(&build_menu(&item.submenu, &path, paths, target)));
        }
        menu.addItem(&mi);
    }
    menu
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            start();
        }

        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            if let Some(path) = urls.iter().find_map(|u| u.path()).map(|p| PathBuf::from(p.to_string())) {
                // A skin opened before launch finished becomes the first skin.
                let started = APP.with(|a| a.borrow().media.is_some());
                if started {
                    push(Pending::Load(path));
                } else {
                    APP.with(|a| a.borrow_mut().first_skin = Some(path));
                }
            }
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _n: &NSNotification) {
            APP.with(|a| a.borrow_mut().replacing = true);
            if let Some(s) = current() {
                s.close();
            }
        }
    }

    impl AppDelegate {
        #[unsafe(method(openSkin:))]
        fn open_skin(&self, _sender: Option<&AnyObject>) {
            push(Pending::ChooseSkin);
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

fn main_menu(mtm: MainThreadMarker, delegate: &AppDelegate) -> Retained<NSMenu> {
    let bar = NSMenu::new(mtm);
    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);
    let item = |title: &str, action, key: &str| unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), Some(action), &NSString::from_str(key))
    };
    let open = item("Open Skin…", sel!(openSkin:), "o");
    unsafe { open.setTarget(Some(delegate)) };
    app_menu.addItem(&open);
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&item("Hide WebSkin Player", sel!(hide:), "h"));
    app_menu.addItem(&item("Quit WebSkin Player", sel!(terminate:), "q"));
    app_item.setSubmenu(Some(&app_menu));
    bar.addItem(&app_item);
    bar
}

fn start() {
    APP.with(|a| a.borrow_mut().media = Some(NowPlaying::start()));
    let first = APP.with(|a| a.borrow_mut().first_skin.take()).or_else(|| Settings::load().last_skin.filter(|p| p.exists()));
    push(first.map_or(Pending::ChooseSkin, Pending::Load));
    let block = RcBlock::new(|_timer: std::ptr::NonNull<NSTimer>| tick());
    let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(1.0 / 60.0, true, &block) };
    unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
}

fn tick() {
    let pending = APP.with(|a| std::mem::take(&mut a.borrow_mut().pending));
    for p in pending {
        match p {
            Pending::ChooseSkin => choose_skin(),
            Pending::Load(path) => load_skin(&path),
        }
    }
    let Some(shared) = current() else { return };
    let windows: Vec<(ViewId, Rc<ViewWindow>)> = APP.with(|a| a.borrow().windows.iter().map(|(v, w)| (*v, w.clone())).collect());
    for (v, w) in windows {
        if let Some((left, top)) = w.moved() {
            shared.with(|s| s.window_moved(v, left, top));
        }
    }
    shared.tick(Instant::now());
}

fn choose_skin() {
    let mtm = mtm();
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    panel.setMessage(Some(&NSString::from_str("Choose a Windows Media Player skin (.wmz or .wms)")));
    #[allow(deprecated)]
    panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&[NSString::from_str("wmz"), NSString::from_str("wms")])));
    NSApplication::sharedApplication(mtm).activate();
    let chosen = (panel.runModal() == objc2_app_kit::NSModalResponseOK)
        .then(|| panel.URL())
        .flatten()
        .and_then(|u| u.path())
        .map(|p| PathBuf::from(p.to_string()));
    match chosen {
        Some(path) => load_skin(&path),
        None if current().is_none() => quit(),
        None => {}
    }
}

fn load_skin(path: &Path) {
    if let Some(old) = current() {
        APP.with(|a| a.borrow_mut().replacing = true);
        old.close();
        APP.with(|a| {
            let mut a = a.borrow_mut();
            a.replacing = false;
            a.shared = None;
        });
    }
    let Some(media) = APP.with(|a| a.borrow().media.clone()) else { return };
    let mut settings = Settings::load();
    settings.last_skin = Some(path.to_path_buf());
    match Shared::load(path, Box::new(Source(media)), settings, Rc::new(MacHost), false) {
        Ok(shared) => {
            APP.with(|a| a.borrow_mut().shared = Some(shared.clone()));
            shared.read(|s| s.settings.save());
            shared.open_main_view();
        }
        Err(e) => {
            MacHost.message_box("This skin can't be opened", &format!("{}\n\n{e}", path.display()));
            push(Pending::ChooseSkin);
        }
    }
}

/// Runs the app; `skin` is opened first (otherwise the last skin, or a file chooser).
pub fn run(skin: Option<PathBuf>) {
    let mtm = mtm();
    APP.with(|a| a.borrow_mut().first_skin = skin);
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.setMainMenu(Some(&main_menu(mtm, &delegate)));
    app.activate();
    app.run();
}
