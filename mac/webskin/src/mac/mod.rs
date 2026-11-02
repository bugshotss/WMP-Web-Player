//! The macOS app: skin mode (one window per open skin view), full mode with the Skin Chooser, the
//! menu-bar player, the visualizer screen saver, the update checker, a menu bar with WMP's keyboard
//! shortcuts, and the Now Playing session they all follow.

mod audio_tap;
mod chooser;
mod core_audio;
mod full;
mod nowplaying;
mod saver;
pub mod snapshot;
mod ui;
mod updater;
mod volume;
mod widget;
mod window;

use crate::images::Artwork;
use crate::player::{Command, MediaSnapshot, MediaSource};
use crate::render::Rect;
use crate::settings::{Settings, SkinEffect};
use crate::skin::{AppAction, Host, MenuItem, Shared, ViewId};
use crate::vis::{ALL, Analyzer, AudioRing, VisContext, VisFrame};
use block2::RcBlock;
use nowplaying::{NowPlaying, Source};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSColorPanel, NSColorSpace, NSControlStateValueOff,
    NSControlStateValueOn, NSImage, NSMenu, NSMenuDelegate, NSMenuItem, NSOpenPanel, NSScreen, NSSound, NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSData, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer, NSURL,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use window::ViewWindow;

/// What the player shows: a skin, full mode, or only the menu-bar player (WMP's Ctrl+2, Ctrl+1 and its
/// taskbar mini player).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(crate) enum Mode {
    #[default]
    Skin,
    Full,
    MenuBar,
}

enum Pending {
    ChooseSkin,
    Load(PathBuf),
    Command(String),
}

/// What's playing, refreshed a few times a second for full mode, the menu-bar player and the screen saver.
#[derive(Clone, Default)]
pub(crate) struct NowState {
    pub media: MediaSnapshot,
    pub position: f64,
    pub artwork: Option<Rc<Artwork>>,
    pub art_image: Option<Retained<NSImage>>,
    /// Output volume 0..1.
    pub volume: f64,
    pub mute: bool,
    /// Bumped whenever the track or its artwork changes.
    pub version: u32,
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
    /// What the Mac plays, analysed for the visualizations; started when one first shows.
    audio: Option<Audio>,
    mode: Mode,
    /// The player was showing a skin when it went into the menu bar.
    last_was_skin: bool,
    /// The settings while no skin holds them (a skin keeps its own copy and saves it).
    settings: Option<Settings>,
    now: NowState,
    now_polled: Option<Instant>,
    started: bool,
}

struct Audio {
    /// None when capture couldn't start; the visualizers then see silence.
    _tap: Option<audio_tap::AudioTap>,
    analyzer: Analyzer,
}

impl Audio {
    fn start() -> Audio {
        let ring = AudioRing::shared();
        let tap = audio_tap::AudioTap::start(ring.clone())
            .inspect(|_| crate::log("Audio capture started for the visualizations"))
            .inspect_err(|e| crate::log(&format!("Audio capture failed: {e}")))
            .ok();
        Audio { _tap: tap, analyzer: Analyzer::new(ring) }
    }
}

/// What fills a menu each time it opens.
type MenuBuilder = fn() -> Vec<(MenuItem, AppAction)>;

thread_local! {
    static APP: RefCell<AppState> = RefCell::default();
    static CHOSEN: Cell<Option<isize>> = const { Cell::new(None) };
    /// The menu bar's Skins menu, the delegate its items call, and the library skins they open (by tag).
    static SKINS_MENU: RefCell<Option<Retained<NSMenu>>> = const { RefCell::new(None) };
    static DELEGATE: RefCell<Option<Retained<AppDelegate>>> = const { RefCell::new(None) };
    static LIBRARY: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    /// Menus refilled each time they open, with what fills them.
    static DYNAMIC_MENUS: RefCell<Vec<(Retained<NSMenu>, MenuBuilder)>> = const { RefCell::new(Vec::new()) };
    /// The visualization a Custom Color… panel recolours.
    static COLOR_TARGET: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The open skin.
pub(crate) fn current() -> Option<Rc<Shared>> {
    APP.with(|a| a.borrow().shared.clone())
}

fn window_for(v: ViewId) -> Option<Rc<ViewWindow>> {
    APP.with(|a| a.borrow().windows.get(&v).cloned())
}

fn skin_windows() -> Vec<Rc<ViewWindow>> {
    APP.with(|a| a.borrow().windows.values().cloned().collect())
}

fn push(p: Pending) {
    APP.with(|a| a.borrow_mut().pending.push(p));
}

/// Runs one of the app's commands on the next tick (outside whatever is calling now).
pub(crate) fn push_command(command: &str) {
    push(Pending::Command(command.to_string()));
}

pub(crate) fn mode() -> Mode {
    APP.with(|a| a.borrow().mode)
}

pub(crate) fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("the app runs on the main thread")
}

fn quit() {
    let app = NSApplication::sharedApplication(mtm());
    app.terminate(None);
}

// ---- settings ------------------------------------------------------------------------------

/// Reads the settings: the open skin's copy, or the app's while no skin is open.
pub(crate) fn settings<R>(f: impl FnOnce(&Settings) -> R) -> R {
    let mut f = Some(f);
    if let Some(shared) = current()
        && let Some(r) = shared.try_read(|s| f.take().map(|f| f(&s.settings)))
        && let Some(r) = r
    {
        return r;
    }
    let f = f.take().expect("not run yet");
    APP.with(|a| f(a.borrow_mut().settings.get_or_insert_with(Settings::load)))
}

/// Changes and saves the settings: through the open skin when there is one, so its copy stays the one saved.
pub(crate) fn update_settings<R>(f: impl FnOnce(&mut Settings) -> R) -> R {
    let mut f = Some(f);
    if let Some(shared) = current()
        && shared.is_idle()
    {
        return shared.with(|s| {
            let r = (f.take().expect("not run yet"))(&mut s.settings);
            s.settings.save();
            r
        });
    }
    let f = f.take().expect("not run yet");
    APP.with(|a| {
        let mut a = a.borrow_mut();
        let s = a.settings.get_or_insert_with(Settings::load);
        let r = f(s);
        s.save();
        r
    })
}

// ---- what's playing ------------------------------------------------------------------------

fn source() -> Option<Source> {
    APP.with(|a| a.borrow().media.clone()).map(Source)
}

pub(crate) fn now() -> NowState {
    APP.with(|a| a.borrow().now.clone())
}

fn refresh_now(force: bool) {
    let due = APP.with(|a| a.borrow().now_polled.is_none_or(|t| t.elapsed() >= Duration::from_millis(250)));
    if !due && !force {
        return;
    }
    let Some(src) = source() else { return };
    let media = src.snapshot();
    let position = src.position();
    let (volume, mute) = (src.volume(), src.mute());
    let (old_art, old_track) = APP.with(|a| {
        let a = a.borrow();
        (a.now.media.artwork_version, (a.now.media.title.clone(), a.now.media.artist.clone(), a.now.media.source_app.clone()))
    });
    let art_changed = media.artwork_version != old_art;
    let track_changed = old_track != (media.title.clone(), media.artist.clone(), media.source_app.clone());
    let artwork = if art_changed { src.artwork() } else { APP.with(|a| a.borrow().now.artwork.clone()) };
    let art_image = if art_changed { artwork.as_ref().and_then(|a| artwork_image(a)) } else { APP.with(|a| a.borrow().now.art_image.clone()) };
    APP.with(|a| {
        let mut a = a.borrow_mut();
        a.now_polled = Some(Instant::now());
        let n = &mut a.now;
        if art_changed || track_changed {
            n.version = n.version.wrapping_add(1);
        }
        n.media = media;
        n.position = position;
        n.artwork = artwork;
        n.art_image = art_image;
        n.volume = volume;
        n.mute = mute;
    });
}

fn artwork_image(art: &Artwork) -> Option<Retained<NSImage>> {
    let mut pm = tiny_skia::Pixmap::new(art.width, art.height)?;
    for (dst, &c) in pm.pixels_mut().iter_mut().zip(&art.argb) {
        if let Some(px) = tiny_skia::PremultipliedColorU8::from_rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 255) {
            *dst = px;
        }
    }
    ui::ns_image(&pm, (f64::from(art.width), f64::from(art.height)))
}

/// The track as a visualization outside a skin sees it.
pub(crate) fn vis_context() -> VisContext {
    let n = now();
    VisContext { title: n.media.title, artist: n.media.artist, artwork: n.artwork, version: n.version }
}

/// A transport or volume command, from the menus, full mode or the menu-bar player.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Play {
    Toggle,
    Stop,
    Previous,
    Next,
    Mute,
    VolumeUp,
    VolumeDown,
    Shuffle,
    Repeat,
    /// 0..1.
    Volume(f64),
    Seek(f64),
}

/// Sends a command to what's playing: through the open skin's player, so the skin hears of it.
pub(crate) fn play(cmd: Play) {
    if let Some(shared) = current()
        && shared.is_idle()
    {
        shared.player(|p| match cmd {
            Play::Toggle => p.toggle_play_pause(),
            Play::Stop => p.stop(),
            Play::Previous => p.previous(),
            Play::Next => p.next(),
            Play::Mute => p.set_mute(!p.mute()),
            Play::VolumeUp => p.set_volume(p.volume() + 5),
            Play::VolumeDown => p.set_volume(p.volume() - 5),
            Play::Shuffle => p.set_shuffle(!p.shuffle()),
            Play::Repeat => p.set_repeat(!p.repeat()),
            Play::Volume(v) => p.set_volume((v * 100.0).round() as i32),
            Play::Seek(s) => p.set_position(s),
        });
    } else if let Some(src) = source() {
        match cmd {
            Play::Toggle => src.command(Command::TogglePlayPause),
            Play::Stop => src.command(Command::Stop),
            Play::Previous => src.command(Command::Previous),
            Play::Next => src.command(Command::Next),
            Play::Mute => src.set_mute(!src.mute()),
            Play::VolumeUp => src.set_volume((src.volume() + 0.05).min(1.0)),
            Play::VolumeDown => src.set_volume((src.volume() - 0.05).max(0.0)),
            Play::Shuffle => src.set_shuffle(!src.snapshot().shuffle),
            Play::Repeat => src.set_repeat(!src.snapshot().repeat),
            Play::Volume(v) => src.set_volume(v.clamp(0.0, 1.0)),
            Play::Seek(s) => src.seek(s.max(0.0)),
        }
    }
    refresh_now(true);
}

// ---- visualizations ------------------------------------------------------------------------

/// The player-wide visualization and preset.
pub(crate) fn global_vis() -> (usize, usize) {
    if let Some(r) = current().and_then(|s| s.try_read(|s| s.global_vis())) {
        return r;
    }
    settings(|s| {
        let effect = crate::vis::find(&s.vis_effect).unwrap_or(0);
        (effect, s.vis_preset.min(ALL[effect].presets.len() - 1))
    })
}

/// Picked from the player's own lists: every visualizer switches to it. With no skin open (full mode), the
/// choice goes to the skin last used, so it's the one that skin shows next.
pub(crate) fn choose_vis(effect: usize, preset: usize) {
    let effect = effect.min(ALL.len() - 1);
    let preset = preset.min(ALL[effect].presets.len() - 1);
    if let Some(shared) = current()
        && shared.is_idle()
    {
        shared.with(|s| s.choose_vis(effect, preset));
        return;
    }
    update_settings(|s| {
        s.vis_effect = ALL[effect].id.to_string();
        s.vis_preset = preset;
        if let Some(key) = s.last_skin.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_lowercase()) {
            s.skin_effects.insert(key, SkinEffect { effect: ALL[effect].id.to_string(), preset });
        }
    });
}

pub(crate) fn vis_colors(effect: usize) -> String {
    settings(|s| s.vis_colors.get(ALL[effect.min(ALL.len() - 1)].id).cloned().unwrap_or_default())
}

pub(crate) fn set_vis_colors(effect: usize, spec: &str) {
    if let Some(shared) = current()
        && shared.is_idle()
    {
        shared.with(|s| s.set_vis_colors(effect, spec));
        return;
    }
    update_settings(|s| {
        let id = ALL[effect.min(ALL.len() - 1)].id.to_string();
        if spec.is_empty() {
            s.vis_colors.remove(&id);
        } else {
            s.vis_colors.insert(id, spec.to_string());
        }
    });
}

/// The Visualizations list: every visualization's presets, the one showing checked.
pub(crate) fn vis_menu() -> Vec<(MenuItem, AppAction)> {
    let (shown, shown_preset) = global_vis();
    ALL.iter()
        .enumerate()
        .map(|(i, d)| {
            let (items, actions) = d
                .presets
                .iter()
                .enumerate()
                .map(|(p, name)| (MenuItem::item(name, true, i == shown && p == shown_preset), AppAction::Visualization(i, p)))
                .unzip();
            (MenuItem::submenu(d.title, items), AppAction::Submenu(actions))
        })
        .collect()
}

/// "Colors for <visualization>" as a submenu entry.
pub(crate) fn colors_entry() -> (MenuItem, AppAction) {
    let (shown, _) = global_vis();
    let (items, actions) = crate::skin::colors_menu(&vis_colors(shown)).into_iter().unzip();
    (MenuItem::submenu(&format!("Colors for {}", ALL[shown].title), items), AppAction::Submenu(actions))
}

/// The visualizer's right-click menu outside skins: visualizations, colours, and the screen saver.
pub(crate) fn visualizer_menu() -> Vec<(MenuItem, AppAction)> {
    let (items, actions) = vis_menu().into_iter().unzip();
    let mut list = vec![(MenuItem::submenu("Visualizations", items), AppAction::Submenu(actions)), colors_entry()];
    list.push((MenuItem::separator(), AppAction::None));
    list.push(saver::menu_entry());
    list
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinColorTarget"]
    struct ColorTarget;

    impl ColorTarget {
        #[unsafe(method(colorChanged:))]
        fn color_changed(&self, panel: &NSColorPanel) {
            let Some(effect) = COLOR_TARGET.with(Cell::get) else { return };
            let Some(c) = panel.color().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else { return };
            let ch = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
            let hex = format!("#{:02X}{:02X}{:02X}", ch(c.redComponent()), ch(c.greenComponent()), ch(c.blueComponent()));
            set_vis_colors(effect, &format!("custom:{hex}"));
        }
    }
);

/// Custom Color…: the Mac's colour panel, recolouring the visualization showing as the colour changes.
fn pick_custom_color() {
    let mtm = mtm();
    let (effect, _) = global_vis();
    COLOR_TARGET.with(|c| c.set(Some(effect)));
    let panel = NSColorPanel::sharedColorPanel(mtm);
    panel.setShowsAlpha(false);
    if let Some(hex) = vis_colors(effect).strip_prefix("custom:").and_then(crate::vis::colors::parse_hex) {
        panel.setColor(&ui::color(hex, 1.0));
    }
    thread_local! {
        static TARGET: RefCell<Option<Retained<ColorTarget>>> = const { RefCell::new(None) };
    }
    let target = TARGET.with(|t| t.borrow_mut().get_or_insert_with(|| unsafe { msg_send![ColorTarget::alloc(mtm), init] }).clone());
    unsafe {
        panel.setTarget(Some(&target));
        panel.setAction(Some(sel!(colorChanged:)));
    }
    panel.setTitle(&NSString::from_str(&format!("Colors for {}", ALL[effect].title)));
    NSApplication::sharedApplication(mtm).activate();
    panel.makeKeyAndOrderFront(None);
}

// ---- skin windows --------------------------------------------------------------------------

/// Renders a view into its window.
pub(crate) fn redraw(v: ViewId) {
    let (Some(shared), Some(w)) = (current(), window_for(v)) else { return };
    let scale = w.scale();
    let canvas = shared.read(|s| s.views.get(v).and_then(Option::as_ref).and_then(|_| s.render_view(v, scale)));
    if let Some(c) = canvas {
        w.present(c);
    }
}

fn redraw_region(v: ViewId, area: Rect) {
    let (Some(shared), Some(w)) = (current(), window_for(v)) else { return };
    let scale = w.scale();
    let part = shared.read(|s| s.views.get(v).and_then(Option::as_ref).and_then(|_| s.render_region(v, scale, area)));
    if !part.is_some_and(|p| w.present_region(&p)) {
        redraw(v);
    }
}

pub(crate) fn always_on_top() -> bool {
    settings(|s| s.always_on_top)
}

struct MacHost;

impl Host for MacHost {
    fn open_url(&self, url: &str) {
        open_url(url);
    }

    fn view_opened(&self, v: ViewId) {
        let Some(shared) = current() else { return };
        let (title, left, top, w, h) = shared.read(|s| (s.get_str(v, 0, "title"), s.view(v).left, s.view(v).top, s.width(v, 0), s.height(v, 0)));
        let shown = mode() != Mode::MenuBar;
        let window = Rc::new(ViewWindow::open(mtm(), v, &title, left, top, f64::from(w), f64::from(h), always_on_top(), shown));
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

    fn redraw_region(&self, v: ViewId, area: Rect) {
        redraw_region(v, area);
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
        message_box(title, text);
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
        if replacing {
            return;
        }
        // The skin's own close button (or a script) closed it: in skin mode that quits, as in WMP.
        APP.with(|a| {
            let mut a = a.borrow_mut();
            a.shared = None;
            a.settings = None;
        });
        if mode() == Mode::Skin {
            quit();
        }
    }

    fn choose_skin(&self) {
        push(Pending::ChooseSkin);
    }

    fn menu_extras(&self) -> Vec<(MenuItem, AppAction)> {
        let mut list = vec![
            (MenuItem::item("Full Mode", true, false).with_shortcut("cmd+1"), AppAction::Host("fullmode".into())),
            (MenuItem::item("Menu Bar Player", true, false).with_shortcut("cmd+3"), AppAction::Host("menubarmode".into())),
            (MenuItem::item("Always on Top", true, always_on_top()).with_shortcut("opt+cmd+t"), AppAction::Host("ontop".into())),
            saver::menu_entry(),
        ];
        list.extend(updater::menu_entries());
        list
    }

    fn host_action(&self, command: &str) {
        push_command(command);
    }
}

pub(crate) fn open_url(url: &str) {
    if let Some(u) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&u);
    }
}

pub(crate) fn message_box(title: &str, text: &str) {
    let alert = NSAlert::new(mtm());
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(text));
    alert.runModal();
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
                let started = APP.with(|a| a.borrow().started);
                if started {
                    push(Pending::Load(path));
                } else {
                    APP.with(|a| a.borrow_mut().first_skin = Some(path));
                }
            }
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_reopen(&self, _app: &NSApplication, visible: bool) -> bool {
            // Clicking the Dock icon brings the player back out of the menu bar.
            if !visible && mode() == Mode::MenuBar {
                push_command("restore");
            }
            true
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _n: &NSNotification) {
            full::save_frame();
            saver::stop(false);
            APP.with(|a| a.borrow_mut().replacing = true);
            if let Some(s) = current() {
                s.close();
            }
        }
    }

    unsafe impl NSMenuDelegate for AppDelegate {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            refill(menu);
        }
    }

    impl AppDelegate {
        #[unsafe(method(openLibrarySkin:))]
        fn open_library_skin(&self, sender: &NSMenuItem) {
            if let Some(path) = LIBRARY.with(|l| usize::try_from(sender.tag()).ok().and_then(|i| l.borrow().get(i).cloned())) {
                push(Pending::Load(path));
            }
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

fn delegate() -> Option<Retained<AppDelegate>> {
    DELEGATE.with(|d| d.borrow().clone())
}

/// Refills a menu registered with [`dynamic_menu`] (its checkmarks and lists follow the player).
fn refill(menu: &NSMenu) {
    let builder = DYNAMIC_MENUS.with(|d| d.borrow().iter().find(|(m, _)| std::ptr::eq(&**m, menu)).map(|(_, b)| *b));
    if let Some(build) = builder {
        ui::fill_menu(menu, build());
    } else if widget::owns_menu(menu) {
        widget::refill_menu(menu);
    }
}

/// A menu whose items `build` makes each time it opens (built once now, so its key equivalents work).
fn dynamic_menu(mtm: MainThreadMarker, title: &str, build: MenuBuilder) -> Retained<NSMenu> {
    let menu = ui::new_menu(mtm, title, build());
    if let Some(d) = delegate() {
        menu.setDelegate(Some(ProtocolObject::from_ref(&*d)));
    }
    DYNAMIC_MENUS.with(|d| d.borrow_mut().push((menu.clone(), build)));
    menu
}

pub(crate) fn set_menu_delegate(menu: &NSMenu) {
    if let Some(d) = delegate() {
        menu.setDelegate(Some(ProtocolObject::from_ref(&*d)));
    }
}

fn host(command: &str) -> AppAction {
    AppAction::Host(command.into())
}

fn app_menu() -> Vec<(MenuItem, AppAction)> {
    let mut list = vec![
        (MenuItem::item("About WebSkin Player", true, false), host("about")),
        (MenuItem::item("Check for Updates…", true, false), host("update:check")),
    ];
    list.extend(updater::menu_entries());
    list.extend([
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Hide WebSkin Player", true, false).with_shortcut("cmd+h"), host("hide")),
        (MenuItem::item("Hide Others", true, false).with_shortcut("opt+cmd+h"), host("hideothers")),
        (MenuItem::item("Show All", true, false), host("showall")),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Quit WebSkin Player", true, false).with_shortcut("cmd+q"), AppAction::Quit),
    ]);
    list
}

fn file_menu() -> Vec<(MenuItem, AppAction)> {
    vec![
        (MenuItem::item("Open Skin…", true, false).with_shortcut("cmd+o"), AppAction::ChooseSkin),
        (MenuItem::item("Add Skins…", true, false), host("addskins")),
        (MenuItem::item("Show Skins Folder", true, false), host("skinsfolder")),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Skin Chooser…", true, false).with_shortcut("shift+cmd+o"), host("chooser")),
        (MenuItem::item("Get More Skins…", true, false), host("getskins")),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Close Window", true, false).with_shortcut("cmd+w"), host("closewindow")),
    ]
}

/// Play, as WMP's Ctrl+P / Ctrl+S / Ctrl+B / Ctrl+F (with ⌘), shuffle and repeat (Ctrl+H, Ctrl+T), and
/// volume as the Mac's own players do it.
fn controls_menu() -> Vec<(MenuItem, AppAction)> {
    let n = now();
    let m = &n.media;
    let playing = m.status == crate::player::MediaStatus::Playing;
    vec![
        (MenuItem::item(if playing { "Pause" } else { "Play" }, true, false).with_shortcut("cmd+p"), host("play")),
        (MenuItem::item("Stop", true, false).with_shortcut("cmd+s"), host("stop")),
        (MenuItem::item("Previous", true, false).with_shortcut("cmd+b"), host("previous")),
        (MenuItem::item("Next", true, false).with_shortcut("cmd+f"), host("next")),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Shuffle", m.can_shuffle, m.shuffle).with_shortcut("shift+cmd+h"), host("shuffle")),
        (MenuItem::item("Repeat", m.can_repeat, m.repeat).with_shortcut("cmd+t"), host("repeat")),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Volume Up", true, false).with_shortcut("cmd+up"), host("volup")),
        (MenuItem::item("Volume Down", true, false).with_shortcut("cmd+down"), host("voldown")),
        (MenuItem::item("Mute", true, n.mute).with_shortcut("opt+cmd+down"), host("mute")),
    ]
}

fn view_menu() -> Vec<(MenuItem, AppAction)> {
    let mode = mode();
    let (vis_items, vis_actions) = vis_menu().into_iter().unzip();
    let keep = settings(|s| s.menu_bar_always);
    vec![
        (MenuItem::item("Full Mode", true, mode == Mode::Full).with_shortcut("cmd+1"), host("fullmode")),
        (MenuItem::item("Skin Mode", true, mode == Mode::Skin).with_shortcut("cmd+2"), host("skinmode")),
        (MenuItem::item("Menu Bar Player", true, mode == Mode::MenuBar).with_shortcut("cmd+3"), host("menubarmode")),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::submenu("Visualizations", vis_items), AppAction::Submenu(vis_actions)),
        colors_entry(),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Always on Top", true, always_on_top()).with_shortcut("opt+cmd+t"), host("ontop")),
        (MenuItem::item("Keep Player in Menu Bar", true, keep), host("menubar:keep")),
        saver::menu_entry(),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Enter Full Screen", mode == Mode::Full, false).with_shortcut("ctrl+cmd+f"), host("fullscreen")),
    ]
}

fn window_menu() -> Vec<(MenuItem, AppAction)> {
    vec![
        (MenuItem::item("Minimize", true, false).with_shortcut("cmd+m"), host("minimize")),
        (MenuItem::item("Bring All to Front", true, false), host("front")),
    ]
}

fn help_menu() -> Vec<(MenuItem, AppAction)> {
    vec![
        (MenuItem::item("WebSkin Player on GitHub", true, false), host("github")),
        (MenuItem::item("Skins on archive.org", true, false), host("archive")),
    ]
}

fn main_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let bar = NSMenu::new(mtm);
    let add = |menu: Retained<NSMenu>| {
        let item = NSMenuItem::new(mtm);
        item.setSubmenu(Some(&menu));
        bar.addItem(&item);
    };
    add(dynamic_menu(mtm, "WebSkin Player", app_menu));
    add(dynamic_menu(mtm, "File", file_menu));
    let skins_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Skins"));
    SKINS_MENU.with(|m| *m.borrow_mut() = Some(skins_menu.clone()));
    add(skins_menu);
    add(dynamic_menu(mtm, "Controls", controls_menu));
    add(dynamic_menu(mtm, "View", view_menu));
    let window = dynamic_menu(mtm, "Window", window_menu);
    NSApplication::sharedApplication(mtm).setWindowsMenu(Some(&window));
    add(window);
    add(dynamic_menu(mtm, "Help", help_menu));
    bar
}

fn file_url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

fn library_item(title: &str, action: Option<objc2::runtime::Sel>, tag: isize) -> Retained<NSMenuItem> {
    let mtm = mtm();
    let title = NSString::from_str(title);
    let mi = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &title, action, &NSString::new()) };
    if let Some(delegate) = delegate() {
        unsafe { mi.setTarget(Some(&delegate)) };
    }
    mi.setTag(tag);
    mi
}

/// Fills the Skins menu from the library: every skin (under its first letter when there are many), then
/// Add Skins… and Show Skins Folder.
pub(crate) fn refresh_skins_menu() {
    let mtm = mtm();
    let Some(menu) = SKINS_MENU.with(|m| m.borrow().clone()) else { return };
    let skins = crate::library::list(&crate::library::folder());
    menu.removeAllItems();
    if skins.is_empty() {
        menu.addItem(&library_item("No skins yet", None, 0));
    }
    let mut letters: Vec<(String, Retained<NSMenu>)> = Vec::new();
    for (i, path) in skins.iter().enumerate() {
        let name = crate::library::name(path);
        let skin = library_item(&name, Some(sel!(openLibrarySkin:)), i as isize);
        if skins.len() <= 30 {
            menu.addItem(&skin);
            continue;
        }
        let letter = name.chars().next().filter(|c| c.is_alphabetic()).map_or_else(|| "#".to_string(), |c| c.to_uppercase().to_string());
        if letters.last().is_none_or(|(l, _)| *l != letter) {
            letters.push((letter, NSMenu::new(mtm)));
        }
        if let Some((_, sub)) = letters.last() {
            sub.addItem(&skin);
        }
    }
    for (letter, sub) in letters {
        let header = library_item(&letter, None, 0);
        header.setSubmenu(Some(&sub));
        menu.addItem(&header);
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let more = [("Skin Chooser…", "chooser"), ("Get More Skins…", "getskins"), ("Add Skins…", "addskins"), ("Show Skins Folder", "skinsfolder")];
    for (title, command) in more {
        let item = ui::menu_item(mtm, title, "", move || push_command(command));
        menu.addItem(&item);
    }
    LIBRARY.with(|l| *l.borrow_mut() = skins);
}

// ---- running -------------------------------------------------------------------------------

fn start() {
    APP.with(|a| {
        let mut a = a.borrow_mut();
        a.media = Some(NowPlaying::start());
        a.started = true;
    });
    refresh_now(true);
    updater::start();
    let first = APP.with(|a| a.borrow_mut().first_skin.take());
    let (start_mode, last) = settings(|s| (s.start_mode.clone(), s.last_skin.clone().filter(|p| p.exists())));
    match (first, start_mode.as_str(), last) {
        (Some(skin), _, _) => push(Pending::Load(skin)),
        (None, "full", _) => push_command("fullmode"),
        (None, "menubar", last) => {
            // Back where it was tucked away from: the skin, or full mode.
            let from_skin = settings(|s| s.tucked_from_skin) && last.is_some();
            APP.with(|a| {
                let mut a = a.borrow_mut();
                a.last_was_skin = from_skin;
                a.mode = Mode::MenuBar;
            });
            push_command("menubarmode");
        }
        (None, _, Some(last)) => push(Pending::Load(last)),
        // A new Mac has no skins yet: start where they can be fetched.
        (None, _, None) => push_command("chooser"),
    }
    sync_widget();
    let block = RcBlock::new(|_timer: std::ptr::NonNull<NSTimer>| tick());
    let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(1.0 / 60.0, true, &block) };
    unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
    snapshot::begin();
}

fn tick() {
    let pending = APP.with(|a| std::mem::take(&mut a.borrow_mut().pending));
    for p in pending {
        match p {
            Pending::ChooseSkin => choose_skin(),
            Pending::Load(path) => load_skin(&path),
            Pending::Command(c) => run_command(&c),
        }
    }
    refresh_now(false);
    let shared = current();
    let windows: Vec<(ViewId, Rc<ViewWindow>)> = APP.with(|a| a.borrow().windows.iter().map(|(v, w)| (*v, w.clone())).collect());
    let scale = windows.iter().map(|(_, w)| f64::from(w.scale())).fold(1.0, f64::max);
    if let Some(shared) = &shared {
        for (v, w) in &windows {
            if let Some((left, top)) = w.moved() {
                shared.with(|s| s.window_moved(*v, left, top));
            }
        }
    }
    // One analysis of what's playing feeds every visualizer showing.
    let skin_wants = shared.as_ref().is_some_and(|s| s.wants_visualization()) && mode() != Mode::MenuBar;
    let full_wants = full::wants_frame();
    let saver_wants = saver::is_running();
    let widget_wants = widget::wants_frame();
    if skin_wants || full_wants || saver_wants || widget_wants {
        // Out of APP while the skin runs: its scripts may call back into the host.
        let mut audio = APP.with(|a| a.borrow_mut().audio.take()).unwrap_or_else(Audio::start);
        let frame: &VisFrame = audio.analyzer.analyze();
        if skin_wants && let Some(shared) = &shared {
            shared.render_visualizations(frame, scale);
        }
        if full_wants {
            full::render(frame);
        }
        if saver_wants {
            saver::render(frame);
        }
        if widget_wants {
            widget::render(frame);
        }
        APP.with(|a| a.borrow_mut().audio = Some(audio));
    }
    if let Some(shared) = shared {
        shared.tick(Instant::now());
    }
    full::tick();
    widget::tick();
    saver::tick();
    updater::tick();
    snapshot::tick();
}

// ---- modes ---------------------------------------------------------------------------------

/// Closes the open skin without quitting (another mode or skin takes over).
fn close_skin() {
    if let Some(old) = current() {
        APP.with(|a| a.borrow_mut().replacing = true);
        old.close();
        APP.with(|a| {
            let mut a = a.borrow_mut();
            a.replacing = false;
            a.shared = None;
            // The skin saved its settings as it closed: read them afresh.
            a.settings = None;
        });
    }
}

fn set_mode(mode: Mode) {
    APP.with(|a| a.borrow_mut().mode = mode);
    let name = match mode {
        Mode::Skin => "skin",
        Mode::Full => "full",
        Mode::MenuBar => "menubar",
    };
    update_settings(|s| s.start_mode = name.into());
    sync_widget();
}

/// Ctrl+1: full mode, closing the skin as WMP did; `tab` picks Now Playing or the Skin Chooser.
fn show_full_mode(tab: Option<full::Tab>) {
    close_skin();
    set_mode(Mode::Full);
    full::show(tab);
}

/// Ctrl+2: the skin last used, or the Skin Chooser when there is none.
fn skin_mode() {
    if current().is_some() {
        // The skin is open already (perhaps tucked into the menu bar): bring it forward.
        restore(Some(true));
        return;
    }
    match settings(|s| s.last_skin.clone()).filter(|p| p.exists()) {
        Some(path) => load_skin(&path),
        None => show_full_mode(Some(full::Tab::Skins)),
    }
}

/// Ctrl+3: tucks the player into the menu bar, hiding its windows.
fn menu_bar_mode() {
    let was = mode();
    if was != Mode::MenuBar {
        let from_skin = was == Mode::Skin && current().is_some();
        APP.with(|a| a.borrow_mut().last_was_skin = from_skin);
        update_settings(|s| s.tucked_from_skin = from_skin);
    }
    for w in skin_windows() {
        w.set_shown(false);
    }
    full::hide();
    set_mode(Mode::MenuBar);
}

/// Brings the player back out of the menu bar (`skin`: None = whatever was showing before).
fn restore(skin: Option<bool>) {
    let to_skin = skin.unwrap_or_else(|| APP.with(|a| a.borrow().last_was_skin));
    if to_skin {
        if current().is_some() {
            set_mode(Mode::Skin);
            for w in skin_windows() {
                w.set_shown(true);
            }
            NSApplication::sharedApplication(mtm()).activate();
        } else {
            skin_mode();
        }
    } else {
        show_full_mode(None);
    }
}

/// The menu-bar player shows when the player is tucked into it, or always if the user keeps it there.
fn sync_widget() {
    let want = mode() == Mode::MenuBar || settings(|s| s.menu_bar_always);
    if want {
        widget::show();
    } else {
        widget::hide();
    }
}

fn set_always_on_top(on: bool) {
    update_settings(|s| s.always_on_top = on);
    for w in skin_windows() {
        w.set_on_top(on);
    }
    full::set_on_top(on);
}

/// One of the app's commands, from a menu, a shortcut, a button or a skin's menu.
pub(crate) fn run_command(command: &str) {
    let mtm = mtm();
    let app = NSApplication::sharedApplication(mtm);
    match command {
        "fullmode" => show_full_mode(Some(full::Tab::NowPlaying)),
        "chooser" => show_full_mode(Some(full::Tab::Skins)),
        "getskins" => show_full_mode(Some(full::Tab::OnlineSkins)),
        "skinmode" => skin_mode(),
        "menubarmode" => menu_bar_mode(),
        "restore" => restore(None),
        "restore:skin" => restore(Some(true)),
        "restore:full" => restore(Some(false)),
        "ontop" => set_always_on_top(!always_on_top()),
        "menubar:keep" => {
            update_settings(|s| s.menu_bar_always = !s.menu_bar_always);
            sync_widget();
        }
        "menubar:hide" => {
            update_settings(|s| s.menu_bar_always = false);
            if mode() == Mode::MenuBar {
                restore(None);
            } else {
                sync_widget();
            }
        }
        "fullscreen" => full::toggle_full_screen(),
        "play" => play(Play::Toggle),
        "stop" => play(Play::Stop),
        "previous" => play(Play::Previous),
        "next" => play(Play::Next),
        "mute" => play(Play::Mute),
        "volup" => play(Play::VolumeUp),
        "voldown" => play(Play::VolumeDown),
        "shuffle" => play(Play::Shuffle),
        "repeat" => play(Play::Repeat),
        "colors:custom" => pick_custom_color(),
        "addskins" => add_skins(),
        "skinsfolder" => {
            let folder = crate::library::folder();
            let _ = std::fs::create_dir_all(&folder);
            NSWorkspace::sharedWorkspace().openURL(&file_url(&folder));
        }
        "about" => {
            app.activate();
            app.orderFrontStandardAboutPanel(None);
        }
        "hide" => app.hide(None),
        "hideothers" => app.hideOtherApplications(None),
        "showall" => app.unhideAllApplications(None),
        "closewindow" => {
            if let Some(w) = app.keyWindow() {
                w.performClose(None);
            }
        }
        "minimize" => {
            if let Some(w) = app.keyWindow() {
                w.miniaturize(None);
            }
        }
        "front" => app.arrangeInFront(None),
        "github" => open_url("https://github.com/bugshotss/WMP-Web-Player"),
        "archive" => open_url(crate::online::PAGE_URL),
        "quit" => quit(),
        c if c.starts_with("saver:") => saver::command(c),
        c if c.starts_with("update:") => updater::command(c),
        other => crate::log(&format!("Unknown command {other}")),
    }
}

/// Runs a menu entry chosen outside a skin (the menu bar, full mode, the menu-bar player).
pub(crate) fn run_app_action(action: &AppAction) {
    match action {
        // On the next tick: a command may close the very menu (or window) it was chosen from.
        AppAction::Host(command) => push_command(command),
        AppAction::Visualization(effect, preset) => choose_vis(*effect, *preset),
        AppAction::VisColors(spec) => set_vis_colors(global_vis().0, spec),
        AppAction::PlayPause => play(Play::Toggle),
        AppAction::Stop => play(Play::Stop),
        AppAction::Previous => play(Play::Previous),
        AppAction::Next => play(Play::Next),
        AppAction::Mute => play(Play::Mute),
        AppAction::VolumeUp => play(Play::VolumeUp),
        AppAction::VolumeDown => play(Play::VolumeDown),
        AppAction::Shuffle => play(Play::Shuffle),
        AppAction::Repeat => play(Play::Repeat),
        AppAction::ChooseSkin => push(Pending::ChooseSkin),
        AppAction::OpenUrl(url) => open_url(url),
        AppAction::Quit => quit(),
        AppAction::None | AppAction::Submenu(_) => {}
    }
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
    let library = crate::library::folder();
    if library.is_dir() {
        panel.setDirectoryURL(Some(&file_url(&library)));
    }
    NSApplication::sharedApplication(mtm).activate();
    let chosen = (panel.runModal() == objc2_app_kit::NSModalResponseOK)
        .then(|| panel.URL())
        .flatten()
        .and_then(|u| u.path())
        .map(|p| PathBuf::from(p.to_string()));
    match chosen {
        Some(path) => load_skin(&path),
        // Nothing open to go back to: the Skin Chooser rather than an empty app.
        None if current().is_none() && !full::is_shown() && mode() != Mode::MenuBar => show_full_mode(Some(full::Tab::Skins)),
        None => {}
    }
}

/// Add Skins…: skins, folders of them and .zip files of them go into the library.
fn add_skins() {
    let mtm = mtm();
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(true);
    panel.setMessage(Some(&NSString::from_str("Choose skins (.wmz), folders of skins, or .zip files of them")));
    NSApplication::sharedApplication(mtm).activate();
    if panel.runModal() != objc2_app_kit::NSModalResponseOK {
        return;
    }
    let library = crate::library::folder();
    let mut added = 0;
    for path in panel.URLs().iter().filter_map(|u| u.path()).map(|p| PathBuf::from(p.to_string())) {
        match crate::library::import(&path, &library) {
            Ok(n) => added += n,
            Err(e) => crate::log(&format!("Adding skins from {} failed: {e}", path.display())),
        }
    }
    crate::log(&format!("Added {added} skins to the library"));
    library_changed();
    let plural = if added == 1 { "" } else { "s" };
    message_box("Skins added", &format!("{added} skin{plural} added to the Skins menu."));
}

/// The library changed (skins added or downloaded): the Skins menu and the Skin Chooser follow.
pub(crate) fn library_changed() {
    refresh_skins_menu();
    full::library_changed();
}

fn load_skin(path: &Path) {
    close_skin();
    let Some(media) = APP.with(|a| a.borrow().media.clone()) else { return };
    // The app's copy of the settings goes to the skin, which keeps and saves them while it's open.
    let mut settings = APP.with(|a| a.borrow_mut().settings.take()).unwrap_or_else(Settings::load);
    settings.last_skin = Some(path.to_path_buf());
    settings.start_mode = "skin".into();
    let was = mode();
    APP.with(|a| a.borrow_mut().mode = Mode::Skin);
    match Shared::load(path, Box::new(Source(media)), settings, Rc::new(MacHost), false) {
        Ok(shared) => {
            APP.with(|a| a.borrow_mut().shared = Some(shared.clone()));
            shared.read(|s| s.settings.save());
            full::hide();
            sync_widget();
            shared.open_main_view();
        }
        Err(e) => {
            APP.with(|a| a.borrow_mut().mode = was);
            message_box("This skin can't be opened", &format!("{}\n\n{e}", path.display()));
            if !full::is_shown() {
                show_full_mode(Some(full::Tab::Skins));
            }
        }
    }
}

/// Runs the app; `skin` is opened first (otherwise the mode it was last in).
pub fn run(skin: Option<PathBuf>) {
    let mtm = mtm();
    APP.with(|a| a.borrow_mut().first_skin = skin);
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = AppDelegate::new(mtm);
    DELEGATE.with(|d| *d.borrow_mut() = Some(delegate.clone()));
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.setMainMenu(Some(&main_menu(mtm)));
    refresh_skins_menu();
    app.activate();
    app.run();
}

/// For the snapshot hook and tests: loads a skin as the user would.
pub(crate) fn open_skin_now(path: &Path) {
    load_skin(path);
}

/// The skin windows open now (for the snapshot hook).
pub(crate) fn skin_window_list() -> Vec<(ViewId, Retained<objc2_app_kit::NSWindow>)> {
    APP.with(|a| a.borrow().windows.iter().map(|(v, w)| (*v, Retained::into_super(w.window.clone()))).collect())
}
