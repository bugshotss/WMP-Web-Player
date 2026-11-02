//! Full mode (WMP's Ctrl+1): Now Playing with a large visualizer (double-click for full screen), the song
//! over it, the visualization and preset pickers and colours, a transport bar with seek, shuffle, repeat
//! and volume, and the Skin Chooser in a second tab. Laid out as the Windows app's full mode.

use super::chooser::Chooser;
use super::ui::{self, Flipped, Pin, rect};
use super::{Play, global_vis, mtm, now, play, push_command, settings, update_settings};
use crate::vis::{ALL, Renderer, VisFrame};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSBackingStoreType, NSButton, NSEvent, NSFloatingWindowLevel, NSImageScaling,
    NSImageView, NSNormalWindowLevel, NSPopUpButton, NSResponder, NSSegmentSwitchTracking, NSSegmentedControl, NSSlider, NSTextField, NSView,
    NSWindow, NSWindowDelegate, NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSString};
use objc2_quartz_core::{kCAFilterLinear, kCAGravityResize};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    NowPlaying,
    Skins,
    /// The Skin Chooser's "Get more skins" list.
    OnlineSkins,
}

const CHROME: u32 = 0x1E2329;
const TRANSPORT: u32 = 0x161A1F;
const TEXT: u32 = 0xE6EAF0;
const DIM: u32 = 0x8B95A3;

define_class!(
    /// The big visualizer: shows the frames full mode renders, double-click for full screen, right-click
    /// for the visualizations, colours and the screen saver.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinVisualizerView"]
    pub struct VisView;

    impl VisView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if event.clickCount() == 2 {
                toggle_full_screen();
            }
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        /// Esc leaves full screen, as in the Windows app.
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let full_screen = self.window().is_some_and(|w| w.styleMask().contains(NSWindowStyleMask::FullScreen));
            if event.keyCode() == 53 && full_screen {
                toggle_full_screen();
            } else {
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            let p = self.convertPoint_fromView(event.locationInWindow(), None);
            ui::popup_menu(self, p, super::visualizer_menu());
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinFullModeDelegate"]
    struct FullDelegate;

    unsafe impl NSObjectProtocol for FullDelegate {}

    unsafe impl NSWindowDelegate for FullDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            save_frame();
            // Closing full mode while it is the player quits, as in the Windows app.
            if super::mode() == super::Mode::Full {
                push_command("quit");
            }
        }
    }
);

pub struct FullMode {
    pub window: Retained<NSWindow>,
    _delegate: Retained<FullDelegate>,
    tabs: Retained<NSSegmentedControl>,
    now_panel: Retained<Flipped>,
    pub chooser: Rc<Chooser>,
    vis: Retained<VisView>,
    title: Retained<NSTextField>,
    artist: Retained<NSTextField>,
    hint: Retained<NSTextField>,
    effect_box: Retained<NSPopUpButton>,
    preset_box: Retained<NSPopUpButton>,
    update: Retained<NSButton>,
    colors: Retained<NSButton>,
    seek: Retained<NSSlider>,
    position: Retained<NSTextField>,
    duration: Retained<NSTextField>,
    art: Retained<NSImageView>,
    bar_title: Retained<NSTextField>,
    bar_status: Retained<NSTextField>,
    shuffle: Retained<NSButton>,
    stop: Retained<NSButton>,
    previous: Retained<NSButton>,
    play: Retained<NSButton>,
    next: Retained<NSButton>,
    repeat: Retained<NSButton>,
    mute: Retained<NSButton>,
    volume: Retained<NSSlider>,
    on_top: Retained<NSButton>,
    renderer: RefCell<Renderer>,
    refreshed: Cell<Option<Instant>>,
    vis_shown: Cell<(usize, usize)>,
}

thread_local! {
    static FULL: RefCell<Option<Rc<FullMode>>> = const { RefCell::new(None) };
}

fn get() -> Option<Rc<FullMode>> {
    FULL.with(|f| f.borrow().clone())
}

pub fn is_shown() -> bool {
    get().is_some_and(|f| f.window.isVisible())
}

/// Something needs the audio analysis: the visualizer can be seen.
pub fn wants_frame() -> bool {
    get()
        .is_some_and(|f| f.window.isVisible() && f.window.occlusionState().contains(NSWindowOcclusionState::Visible) && f.tabs.selectedSegment() == 0)
}

fn build(mtm: MainThreadMarker) -> Rc<FullMode> {
    let (w, h) = (900.0, 620.0);
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable | NSWindowStyleMask::Resizable;
    let window: Retained<NSWindow> = unsafe {
        msg_send![NSWindow::alloc(mtm), initWithContentRect: rect(0.0, 0.0, w, h), styleMask: style, backing: NSBackingStoreType::Buffered, defer: false]
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("WebSkin Player"));
    window.setMinSize(objc2_foundation::NSSize::new(560.0, 400.0));
    window.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).as_deref());
    window.setCollectionBehavior(objc2_app_kit::NSWindowCollectionBehavior::FullScreenPrimary);
    match settings(|s| s.full_mode_frame) {
        Some((x, y, fw, fh)) if fw >= 560.0 && fh >= 400.0 => window.setFrame_display(rect(x, y, fw, fh), false),
        _ => window.center(),
    }
    let delegate: Retained<FullDelegate> = unsafe { msg_send![FullDelegate::alloc(mtm), init] };
    window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    let root = ui::flipped(mtm, rect(0.0, 0.0, w, h), Some(CHROME));
    window.setContentView(Some(&root));

    // Header: the tabs, and Update available on the right.
    let tabs = unsafe {
        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
            &NSArray::from_retained_slice(&[NSString::from_str("Now Playing"), NSString::from_str("Skin Chooser")]),
            NSSegmentSwitchTracking::SelectOne,
            None,
            None,
            mtm,
        )
    };
    tabs.setSelectedSegment(0);
    ui::place(&tabs, &root, rect(12.0, 8.0, 240.0, 24.0), Pin::NEAR_Y);
    ui::on_action(&tabs, || {
        if let Some(f) = get() {
            f.select(if f.tabs.selectedSegment() == 0 { Tab::NowPlaying } else { Tab::Skins });
        }
    });
    let update = ui::text_button(mtm, "Update available", || push_command("update:install"));
    update.setHidden(true);
    ui::place(&update, &root, rect(w - 212.0, 6.0, 200.0, 28.0), Pin::and(Pin::RIGHT, Pin::NEAR_Y));

    // Now Playing: the visualizer, the song over it, and the visualization pickers.
    let main_h = h - 40.0 - 96.0;
    let now_panel = ui::flipped(mtm, rect(0.0, 40.0, w, main_h), Some(0x000000));
    ui::place(&now_panel, &root, rect(0.0, 40.0, w, main_h), Pin::FILL);
    let vis: Retained<VisView> = unsafe { msg_send![VisView::alloc(mtm), initWithFrame: rect(0.0, 0.0, w, main_h)] };
    vis.setWantsLayer(true);
    if let Some(layer) = vis.layer() {
        layer.setContentsGravity(unsafe { kCAGravityResize });
        layer.setMagnificationFilter(unsafe { kCAFilterLinear });
    }
    vis.setToolTip(Some(&NSString::from_str("Double-click for full screen · right-click to choose a visualization")));
    ui::place(&vis, &now_panel, rect(0.0, 0.0, w, main_h), Pin::FILL);
    let shadow = objc2_app_kit::NSShadow::new();
    shadow.setShadowBlurRadius(6.0);
    shadow.setShadowColor(Some(&objc2_app_kit::NSColor::blackColor()));
    let title = ui::label(mtm, "", 20.0, true, 0xFFFFFF);
    let artist = ui::label(mtm, "", 14.0, false, 0xB9C4D2);
    for (l, y, lh) in [(&title, 12.0, 28.0), (&artist, 40.0, 20.0)] {
        l.setShadow(Some(&shadow));
        ui::place(l, &now_panel, rect(16.0, y, w - 32.0, lh), Pin::and(Pin::WIDTH, Pin::NEAR_Y));
    }
    let hint = NSTextField::wrappingLabelWithString(
        &NSString::from_str("Start playing something in Spotify, Music or your browser.\nIt will show up here, and any skin can control it."),
        mtm,
    );
    hint.setAlignment(objc2_app_kit::NSTextAlignment::Center);
    hint.setTextColor(Some(&ui::color(DIM, 1.0)));
    hint.setFont(Some(&ui::font(15.0, false)));
    ui::place(&hint, &now_panel, rect((w - 480.0) / 2.0, main_h / 2.0 - 30.0, 480.0, 60.0), Pin::CENTER);

    let controls = ui::flipped(mtm, rect(0.0, 0.0, 462.0, 34.0), None);
    if let Some(layer) = controls.layer() {
        layer.setBackgroundColor(Some(&objc2_core_graphics::CGColor::new_srgb(0.06, 0.08, 0.1, 0.6)));
        layer.setCornerRadius(5.0);
    }
    ui::place(&controls, &now_panel, rect(w - 462.0 - 12.0, main_h - 34.0 - 12.0, 462.0, 34.0), Pin::and(Pin::RIGHT, Pin::FAR_Y));
    let prev_effect = ui::symbol_button(mtm, "chevron.left", "Previous visualization", || {
        let (effect, _) = global_vis();
        super::choose_vis((effect + ALL.len() - 1) % ALL.len(), 0);
    });
    ui::place(&prev_effect, &controls, rect(4.0, 5.0, 24.0, 24.0), Pin::NEAR_Y);
    let effect_box = ui::popup(mtm, || {});
    ui::place(&effect_box, &controls, rect(30.0, 5.0, 150.0, 24.0), Pin::NEAR_Y);
    let eb = effect_box.clone();
    ui::on_action(&effect_box, move || super::choose_vis(eb.indexOfSelectedItem().max(0) as usize, 0));
    let preset_box = ui::popup(mtm, || {});
    ui::place(&preset_box, &controls, rect(184.0, 5.0, 180.0, 24.0), Pin::NEAR_Y);
    let pb = preset_box.clone();
    ui::on_action(&preset_box, move || super::choose_vis(global_vis().0, pb.indexOfSelectedItem().max(0) as usize));
    let next_preset = ui::symbol_button(mtm, "chevron.right", "Next preset", || {
        let (effect, preset) = global_vis();
        super::choose_vis(effect, (preset + 1) % ALL[effect].presets.len());
    });
    ui::place(&next_preset, &controls, rect(368.0, 5.0, 24.0, 24.0), Pin::NEAR_Y);
    let colors = ui::symbol_button(mtm, "paintpalette", "Colors", || {});
    ui::place(&colors, &controls, rect(400.0, 5.0, 24.0, 24.0), Pin::NEAR_Y);
    let cb = colors.clone();
    ui::on_action(&colors, move || {
        let (item, action) = super::colors_entry();
        if let crate::skin::AppAction::Submenu(list) = action {
            ui::popup_menu(&cb, NSPoint::new(0.0, 26.0), item.submenu.into_iter().zip(list).collect());
        }
    });
    let saver_btn = ui::symbol_button(mtm, "sparkles.tv", "Screen saver", || {});
    ui::place(&saver_btn, &controls, rect(432.0, 5.0, 24.0, 24.0), Pin::NEAR_Y);
    let sb = saver_btn.clone();
    ui::on_action(&saver_btn, move || {
        let (item, action) = super::saver::menu_entry();
        if let crate::skin::AppAction::Submenu(list) = action {
            ui::popup_menu(&sb, NSPoint::new(0.0, 26.0), item.submenu.into_iter().zip(list).collect());
        }
    });

    // The Skin Chooser fills the same area.
    let chooser = Chooser::new(mtm, rect(0.0, 40.0, w, main_h));
    ui::place(&chooser.view, &root, rect(0.0, 40.0, w, main_h), Pin::FILL);
    chooser.view.setHidden(true);

    // The transport bar.
    let bar = ui::flipped(mtm, rect(0.0, h - 96.0, w, 96.0), Some(TRANSPORT));
    ui::place(&bar, &root, rect(0.0, h - 96.0, w, 96.0), Pin::and(Pin::WIDTH, Pin::FAR_Y));
    let position = ui::label(mtm, "", 11.0, false, DIM);
    position.setFont(Some(&objc2_app_kit::NSFont::monospacedDigitSystemFontOfSize_weight(11.0, unsafe { objc2_app_kit::NSFontWeightRegular })));
    ui::place(&position, &bar, rect(12.0, 8.0, 56.0, 16.0), Pin::NEAR_Y);
    let duration = ui::label(mtm, "", 11.0, false, DIM);
    duration.setFont(Some(&objc2_app_kit::NSFont::monospacedDigitSystemFontOfSize_weight(11.0, unsafe { objc2_app_kit::NSFontWeightRegular })));
    duration.setAlignment(objc2_app_kit::NSTextAlignment::Right);
    ui::place(&duration, &bar, rect(w - 68.0, 8.0, 56.0, 16.0), Pin::and(Pin::RIGHT, Pin::NEAR_Y));
    let seek = ui::slider(mtm, 1.0, || {});
    seek.setContinuous(false);
    ui::place(&seek, &bar, rect(72.0, 6.0, w - 144.0, 20.0), Pin::and(Pin::WIDTH, Pin::NEAR_Y));
    let sk = seek.clone();
    ui::on_action(&seek, move || play(Play::Seek(sk.doubleValue())));

    let art = NSImageView::initWithFrame(NSImageView::alloc(mtm), rect(12.0, 42.0, 40.0, 40.0));
    art.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    ui::place(&art, &bar, rect(12.0, 42.0, 40.0, 40.0), Pin::NEAR_Y);
    let bar_title = ui::label(mtm, "", 12.0, true, TEXT);
    ui::place(&bar_title, &bar, rect(60.0, 44.0, 220.0, 17.0), Pin::NEAR_Y);
    let bar_status = ui::label(mtm, "", 11.0, false, DIM);
    ui::place(&bar_status, &bar, rect(60.0, 62.0, 220.0, 16.0), Pin::NEAR_Y);

    let center = ui::flipped(mtm, rect(0.0, 0.0, 380.0, 44.0), None);
    ui::place(&center, &bar, rect((w - 380.0) / 2.0, 40.0, 380.0, 44.0), Pin::and(Pin::CENTER_X, Pin::NEAR_Y));
    let shuffle = ui::toggle_button(mtm, "shuffle", "Shuffle (⇧⌘H)", || play(Play::Shuffle));
    let stop = ui::symbol_button(mtm, "stop.fill", "Stop (⌘S)", || play(Play::Stop));
    let previous = ui::symbol_button(mtm, "backward.fill", "Previous (⌘B)", || play(Play::Previous));
    let play_btn = ui::symbol_button(mtm, "play.circle.fill", "Play (⌘P)", || play(Play::Toggle));
    let next = ui::symbol_button(mtm, "forward.fill", "Next (⌘F)", || play(Play::Next));
    let repeat = ui::toggle_button(mtm, "repeat", "Repeat (⌘T)", || play(Play::Repeat));
    let mute = ui::symbol_button(mtm, "speaker.wave.2.fill", "Mute (⌥⌘↓)", || play(Play::Mute));
    let mut x = 0.0;
    for (b, size) in [(&shuffle, 26.0), (&stop, 26.0), (&previous, 30.0), (&play_btn, 40.0), (&next, 30.0), (&repeat, 26.0), (&mute, 26.0)] {
        b.setContentTintColor(Some(&ui::color(0xC8D0DA, 1.0)));
        ui::place(b, &center, rect(x, (44.0 - size) / 2.0, size, size), Pin::NEAR_Y);
        x += size + 8.0;
    }
    let volume = ui::slider(mtm, 100.0, || {});
    ui::place(&volume, &center, rect(x, 12.0, 380.0 - x, 20.0), Pin::NEAR_Y);
    let vb = volume.clone();
    ui::on_action(&volume, move || play(Play::Volume(vb.doubleValue() / 100.0)));

    let right = ui::flipped(mtm, rect(0.0, 0.0, 104.0, 44.0), None);
    ui::place(&right, &bar, rect(w - 116.0, 40.0, 104.0, 44.0), Pin::and(Pin::RIGHT, Pin::NEAR_Y));
    let on_top = ui::toggle_button(mtm, "pin.fill", "Keep the player above other windows (⌥⌘T)", || push_command("ontop"));
    let menubar = ui::symbol_button(mtm, "menubar.arrow.up.rectangle", "Tuck into the menu bar (⌘3)", || push_command("menubarmode"));
    let skin = ui::symbol_button(mtm, "rectangle.on.rectangle", "Switch to skin mode (⌘2)", || push_command("skinmode"));
    for (i, b) in [&on_top, &menubar, &skin].into_iter().enumerate() {
        b.setContentTintColor(Some(&ui::color(0xC8D0DA, 1.0)));
        ui::place(b, &right, rect(i as f64 * 36.0, 9.0, 28.0, 26.0), Pin::NEAR_Y);
    }

    let mut renderer = Renderer::new(0, 0);
    renderer.max_pixels = 960 * 540;
    let f = Rc::new(FullMode {
        window,
        _delegate: delegate,
        tabs,
        now_panel,
        chooser,
        vis,
        title,
        artist,
        hint,
        effect_box,
        preset_box,
        update,
        colors,
        seek,
        position,
        duration,
        art,
        bar_title,
        bar_status,
        shuffle,
        stop,
        previous,
        play: play_btn,
        next,
        repeat,
        mute,
        volume,
        on_top,
        renderer: RefCell::new(renderer),
        refreshed: Cell::new(None),
        vis_shown: Cell::new((usize::MAX, 0)),
    });
    f.set_on_top(super::always_on_top());
    f
}

impl FullMode {
    fn select(&self, tab: Tab) {
        let skins = tab != Tab::NowPlaying;
        self.tabs.setSelectedSegment(isize::from(skins));
        self.now_panel.setHidden(skins);
        self.chooser.view.setHidden(!skins);
        if skins {
            self.chooser.shown(tab == Tab::OnlineSkins);
        } else {
            self.window.makeFirstResponder(Some(&self.vis));
        }
    }

    fn set_on_top(&self, on: bool) {
        self.window.setLevel(if on { NSFloatingWindowLevel } else { NSNormalWindowLevel });
        ui::show_toggle(&self.on_top, on);
    }

    /// The song, the buttons and the pickers, a few times a second.
    fn refresh(&self) {
        let n = now();
        let m = &n.media;
        let session = m.has_session();
        let playing = m.status == crate::player::MediaStatus::Playing;
        ui::set_text(&self.title, &m.title);
        let artist = if !m.album.is_empty() && !m.artist.is_empty() { format!("{} — {}", m.artist, m.album) } else { m.artist.clone() };
        ui::set_text(&self.artist, &artist);
        self.hint.setHidden(session);
        ui::set_text(
            &self.bar_title,
            &if session { if m.title.is_empty() { m.source_app.clone() } else { m.title.clone() } } else { "Nothing playing".into() },
        );
        let status = if session {
            format!("{} · {}", if playing { "Playing" } else { "Paused" }, m.source_app)
        } else {
            "Waiting for an app or browser".into()
        };
        ui::set_text(&self.bar_status, &status);
        self.art.setImage(n.art_image.as_deref());
        ui::set_symbol(&self.play, if playing { "pause.circle.fill" } else { "play.circle.fill" }, if playing { "Pause (⌘P)" } else { "Play (⌘P)" });
        for b in [&self.play, &self.stop] {
            b.setEnabled(session);
        }
        self.previous.setEnabled(m.can_previous);
        self.next.setEnabled(m.can_next);
        self.shuffle.setEnabled(m.can_shuffle);
        self.repeat.setEnabled(m.can_repeat);
        ui::show_toggle(&self.shuffle, m.shuffle);
        ui::show_toggle(&self.repeat, m.repeat);
        ui::set_symbol(
            &self.mute,
            if n.mute { "speaker.slash.fill" } else { "speaker.wave.2.fill" },
            if n.mute { "Unmute (⌥⌘↓)" } else { "Mute (⌥⌘↓)" },
        );
        self.volume.setDoubleValue((n.volume * 100.0).round());
        self.seek.setEnabled(m.can_seek && m.duration > 0.0);
        // Not while the user drags it.
        if NSEvent::pressedMouseButtons() == 0 {
            self.seek.setMaxValue(m.duration.max(1.0));
            self.seek.setDoubleValue(if m.duration > 0.0 { n.position.min(m.duration) } else { 0.0 });
        }
        ui::set_text(&self.position, &if session { crate::player::format_time(n.position) } else { String::new() });
        ui::set_text(&self.duration, &if m.duration > 0.0 { crate::player::format_time(m.duration) } else { String::new() });
        let window_title = if session && !m.title.is_empty() { format!("{} - WebSkin Player", m.title) } else { "WebSkin Player".into() };
        if self.window.title().to_string() != window_title {
            self.window.setTitle(&NSString::from_str(&window_title));
        }
        let vis = global_vis();
        let spec = super::vis_colors(vis.0);
        let tip = format!("Colors: {}", crate::vis::colors::describe(&spec));
        if self.colors.toolTip().is_none_or(|t| t.to_string() != tip) {
            self.colors.setToolTip(Some(&NSString::from_str(&tip)));
        }
        if self.vis_shown.get() != vis {
            self.vis_shown.set(vis);
            ui::set_items(&self.effect_box, &ALL.iter().map(|d| d.title.to_string()).collect::<Vec<_>>(), vis.0);
            ui::set_items(&self.preset_box, &ALL[vis.0].presets.iter().map(|p| p.to_string()).collect::<Vec<_>>(), vis.1);
        }
        self.show_update();
    }

    fn show_update(&self) {
        let available = super::updater::available();
        self.update.setHidden(available.is_none());
        if let Some(u) = available {
            let text = super::updater::status().unwrap_or_else(|| format!("Update to {}", u.version));
            if self.update.title().to_string() != text {
                self.update.setTitle(&NSString::from_str(&text));
                self.update.setToolTip(Some(&NSString::from_str("Install the new version, or see what's new in it")));
            }
        }
    }

    fn render(&self, frame: &VisFrame) {
        let (effect, preset) = global_vis();
        let size = self.vis.bounds().size;
        let scale = self.window.backingScaleFactor();
        let mut r = self.renderer.borrow_mut();
        r.set(effect, preset);
        let colors = crate::vis::colors::map_for(&super::vis_colors(effect));
        r.render(frame, &super::vis_context(), (size.width * scale) as usize, (size.height * scale) as usize, colors.as_deref());
        let space = self.window.colorSpace().and_then(|s| s.CGColorSpace());
        if let (Some(pm), Some(layer)) = (r.image.as_ref(), self.vis.layer())
            && let Some(image) = ui::cg_image(pm, space.as_deref())
        {
            unsafe { layer.setContents(Some(ui::layer_contents(&image))) };
        }
    }
}

/// Opens full mode (creating it the first time) on `tab` (None: as it was).
pub fn show(tab: Option<Tab>) {
    let mtm = mtm();
    let f = match get() {
        Some(f) => f,
        None => {
            let f = build(mtm);
            FULL.with(|slot| *slot.borrow_mut() = Some(f.clone()));
            // A new Mac has no skins yet: start where they can be fetched.
            let empty = crate::library::list(&crate::library::folder()).is_empty();
            f.select(tab.map(|t| if t == Tab::Skins && empty { Tab::OnlineSkins } else { t }).unwrap_or(Tab::NowPlaying));
            f.refresh();
            f.window.makeKeyAndOrderFront(None);
            objc2_app_kit::NSApplication::sharedApplication(mtm).activate();
            return;
        }
    };
    if let Some(t) = tab {
        let empty = crate::library::list(&crate::library::folder()).is_empty();
        f.select(if t == Tab::Skins && empty { Tab::OnlineSkins } else { t });
    }
    f.refresh();
    if f.window.isMiniaturized() {
        f.window.deminiaturize(None);
    }
    f.window.makeKeyAndOrderFront(None);
    objc2_app_kit::NSApplication::sharedApplication(mtm).activate();
}

pub fn hide() {
    if let Some(f) = get() {
        save_frame();
        if f.window.styleMask().contains(NSWindowStyleMask::FullScreen) {
            f.window.toggleFullScreen(None);
        }
        f.window.orderOut(None);
    }
}

pub fn save_frame() {
    if let Some(f) = get()
        && f.window.isVisible()
        && !f.window.styleMask().contains(NSWindowStyleMask::FullScreen)
    {
        let r = f.window.frame();
        update_settings(|s| s.full_mode_frame = Some((r.origin.x, r.origin.y, r.size.width, r.size.height)));
    }
}

pub fn toggle_full_screen() {
    if let Some(f) = get()
        && f.window.isVisible()
    {
        f.select(Tab::NowPlaying);
        f.window.toggleFullScreen(None);
    }
}

pub fn set_on_top(on: bool) {
    if let Some(f) = get() {
        f.set_on_top(on);
    }
}

pub fn render(frame: &VisFrame) {
    if let Some(f) = get() {
        f.render(frame);
    }
}

pub fn update_changed() {
    if let Some(f) = get() {
        f.show_update();
    }
}

pub fn library_changed() {
    if let Some(f) = get() {
        f.chooser.library_changed();
    }
}

pub fn tick() {
    let Some(f) = get() else { return };
    if !f.window.isVisible() {
        return;
    }
    if f.refreshed.get().is_none_or(|t| t.elapsed() >= Duration::from_millis(250)) {
        f.refreshed.set(Some(Instant::now()));
        f.refresh();
    }
    f.chooser.tick();
}

/// For the snapshot hook.
pub fn window() -> Option<Retained<NSWindow>> {
    get().map(|f| f.window.clone())
}

pub fn select(tab: Tab) {
    if let Some(f) = get() {
        f.select(tab);
    }
}
