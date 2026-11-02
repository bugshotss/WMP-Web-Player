//! The menu-bar player, the Mac's take on the Windows app's taskbar widget (and WMP's taskbar mini player):
//! the artwork and a live mini spectrum in the menu bar with the song's title, a scroll over it for
//! volume, and a menu with the transport buttons, the volume and the player's modes. ⌘3 tucks the
//! player into it; "Keep Player in Menu Bar" shows it in every mode.

use super::ui::{self, rect};
use super::{Mode, Play, mode, mtm, now, play, settings};
use crate::skin::{AppAction, MenuItem};
use crate::vis::VisFrame;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearanceCustomization, NSCellImagePosition, NSColor, NSEvent, NSEventMask, NSImageScaling, NSImageView, NSMenu, NSMenuItem, NSSlider,
    NSStatusBar, NSStatusItem, NSTextField, NSVariableStatusItemLength, NSView,
};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::ptr::NonNull;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

const BARS: usize = 8;

struct Header {
    item: Retained<NSMenuItem>,
    art: Retained<NSImageView>,
    title: Retained<NSTextField>,
    artist: Retained<NSTextField>,
    play: Retained<objc2_app_kit::NSButton>,
    volume: Retained<NSSlider>,
}

struct Widget {
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    header: Header,
    monitor: Option<Retained<AnyObject>>,
    bars: [f32; BARS],
    frames: u32,
    shown: (String, u32, bool),
    art: Option<Pixmap>,
}

thread_local! {
    static WIDGET: RefCell<Option<Widget>> = const { RefCell::new(None) };
}

pub fn is_shown() -> bool {
    WIDGET.with(|w| w.borrow().is_some())
}

/// The spectrum moves only while something plays (so a silent Mac isn't asked to share its audio).
pub fn wants_frame() -> bool {
    is_shown() && now().media.status == crate::player::MediaStatus::Playing
}

pub fn owns_menu(menu: &NSMenu) -> bool {
    WIDGET.with(|w| w.borrow().as_ref().is_some_and(|w| std::ptr::eq(&*w.menu, menu)))
}

fn header(mtm: MainThreadMarker) -> Header {
    let view = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 300.0, 92.0));
    let art = NSImageView::initWithFrame(NSImageView::alloc(mtm), rect(14.0, 14.0, 64.0, 64.0));
    art.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    view.addSubview(&art);
    let title = ui::label(mtm, "Nothing playing", 13.0, true, 0xFFFFFF);
    title.setTextColor(Some(&NSColor::labelColor()));
    title.setFrame(rect(88.0, 58.0, 200.0, 18.0));
    view.addSubview(&title);
    let artist = ui::label(mtm, "", 11.0, false, 0xFFFFFF);
    artist.setTextColor(Some(&NSColor::secondaryLabelColor()));
    artist.setFrame(rect(88.0, 41.0, 200.0, 16.0));
    view.addSubview(&artist);
    let buttons = [("backward.fill", "Previous", Play::Previous), ("play.fill", "Play", Play::Toggle), ("forward.fill", "Next", Play::Next)];
    let mut play_button = None;
    for (i, (symbol, tip, cmd)) in buttons.into_iter().enumerate() {
        let b = ui::symbol_button(mtm, symbol, tip, move || play(cmd));
        b.setFrame(rect(84.0 + i as f64 * 30.0, 10.0, 28.0, 26.0));
        view.addSubview(&b);
        if i == 1 {
            play_button = Some(b);
        }
    }
    let volume = ui::slider(mtm, 100.0, || {});
    volume.setFrame(rect(184.0, 13.0, 104.0, 20.0));
    let v = volume.clone();
    ui::on_action(&volume, move || play(Play::Volume(v.doubleValue() / 100.0)));
    view.addSubview(&volume);
    let item = NSMenuItem::new(mtm);
    item.setView(Some(&view));
    Header { item, art, title, artist, play: play_button.expect("three buttons"), volume }
}

fn menu_items() -> Vec<(MenuItem, AppAction)> {
    let host = |c: &str| AppAction::Host(c.into());
    let (keep, on_top) = settings(|s| (s.menu_bar_always, s.always_on_top));
    let tucked = mode() == Mode::MenuBar;
    vec![
        (MenuItem::item("Open Player", tucked, false), host("restore")),
        (
            MenuItem::item("Skin Mode", true, mode() == Mode::Skin && !tucked).with_shortcut("cmd+2"),
            host(if tucked { "restore:skin" } else { "skinmode" }),
        ),
        (MenuItem::item("Full Mode", true, mode() == Mode::Full).with_shortcut("cmd+1"), host(if tucked { "restore:full" } else { "fullmode" })),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Keep Player in Menu Bar", true, keep), host("menubar:keep")),
        (MenuItem::item("Always on Top", true, on_top), host("ontop")),
        super::saver::menu_entry(),
        (MenuItem::item("Remove from Menu Bar", true, false), host("menubar:hide")),
        (MenuItem::separator(), AppAction::None),
        (MenuItem::item("Quit WebSkin Player", true, false).with_shortcut("cmd+q"), AppAction::Quit),
    ]
}

/// Refills the menu as it opens: the header first, then the choices.
pub fn refill_menu(menu: &NSMenu) {
    ui::fill_menu(menu, menu_items());
    let mtm = MainThreadMarker::from(menu);
    WIDGET.with(|w| {
        if let Some(w) = w.borrow().as_ref() {
            menu.insertItem_atIndex(&w.header.item, 0);
            menu.insertItem_atIndex(&NSMenuItem::separatorItem(mtm), 1);
        }
    });
    refresh(true);
}

pub fn show() {
    if is_shown() {
        return;
    }
    let mtm = mtm();
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    item.setAutosaveName(Some(&NSString::from_str("WebSkinPlayer")));
    if let Some(button) = item.button(mtm) {
        button.setImagePosition(NSCellImagePosition::ImageLeft);
        button.setToolTip(Some(&NSString::from_str("WebSkin Player · scroll for volume")));
    }
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    super::set_menu_delegate(&menu);
    item.setMenu(Some(&menu));
    // A scroll over the item changes the volume, as over the Windows widget.
    let target = item.button(mtm).and_then(|b| b.window());
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let e = unsafe { event.as_ref() };
        let over = target.as_ref().is_some_and(|t| e.window(mtm).is_some_and(|w| std::ptr::eq(&*w, &**t)));
        if over && e.scrollingDeltaY().abs() >= 0.5 {
            play(if e.scrollingDeltaY() > 0.0 { Play::VolumeUp } else { Play::VolumeDown });
            return std::ptr::null_mut();
        }
        event.as_ptr()
    });
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::ScrollWheel, &block) };
    let w = Widget {
        item,
        menu: menu.clone(),
        header: header(mtm),
        monitor,
        bars: [0.0; BARS],
        frames: 0,
        shown: (String::from("\u{0}"), u32::MAX, false),
        art: None,
    };
    WIDGET.with(|slot| *slot.borrow_mut() = Some(w));
    refill_menu(&menu);
    draw_button();
}

pub fn hide() {
    let Some(w) = WIDGET.with(|slot| slot.borrow_mut().take()) else { return };
    if let Some(m) = &w.monitor {
        unsafe { NSEvent::removeMonitor(m) };
    }
    NSStatusBar::systemStatusBar().removeStatusItem(&w.item);
}

/// The spectrum's bands, from bass to treble, eased so they fall back gently.
pub fn render(frame: &VisFrame) {
    const EDGES: [usize; BARS + 1] = [1, 3, 6, 12, 24, 48, 96, 192, 400];
    let redraw = WIDGET.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(w) = slot.as_mut() else { return false };
        for i in 0..BARS {
            let band = &frame.spectrum[EDGES[i]..EDGES[i + 1]];
            let v = band.iter().copied().fold(0.0f32, f32::max).clamp(0.0, 1.0);
            w.bars[i] = v.max(w.bars[i] * 0.85);
        }
        w.frames = w.frames.wrapping_add(1);
        w.frames % 3 == 0
    });
    if redraw {
        draw_button();
    }
}

/// The menu bar is dark (white marks) or light (black ones).
fn dark_menu_bar(mtm: MainThreadMarker) -> bool {
    WIDGET.with(|slot| {
        slot.borrow().as_ref().and_then(|w| w.item.button(mtm)).is_some_and(|b| b.effectiveAppearance().name().to_string().contains("Dark"))
    })
}

/// Draws the artwork and the mini spectrum as the item's picture (2 pixels per point).
fn draw_button() {
    let mtm = mtm();
    let dark = dark_menu_bar(mtm);
    let playing = now().media.has_session();
    let image = WIDGET.with(|slot| {
        let slot = slot.borrow();
        let w = slot.as_ref()?;
        let (pw, ph) = (if playing { 46.0 } else { 18.0 }, 18.0);
        let mut pm = Pixmap::new((pw * 2.0) as u32, (ph * 2.0) as u32)?;
        let ink = if dark { tiny_skia::Color::from_rgba8(255, 255, 255, 230) } else { tiny_skia::Color::from_rgba8(0, 0, 0, 220) };
        let mut paint = Paint { anti_alias: true, ..Default::default() };
        paint.set_color(ink);
        match (&w.art, playing) {
            (Some(art), true) => {
                let mut pb = PathBuilder::new();
                pb.push_rect(tiny_skia::Rect::from_xywh(0.0, 2.0, 32.0, 32.0)?);
                let path = pb.finish()?;
                let k = 32.0 / art.width().max(1) as f32;
                let shader = tiny_skia::Pattern::new(
                    art.as_ref(),
                    tiny_skia::SpreadMode::Pad,
                    tiny_skia::FilterQuality::Bilinear,
                    1.0,
                    Transform::from_scale(k, k).post_translate(0.0, 2.0),
                );
                let art_paint = Paint { shader, anti_alias: true, ..Default::default() };
                pm.fill_path(&path, &art_paint, FillRule::Winding, Transform::identity(), None);
            }
            _ => {
                // A note, in place of artwork.
                let mut pb = PathBuilder::new();
                pb.push_circle(10.0, 27.0, 5.0);
                pb.push_rect(tiny_skia::Rect::from_xywh(13.0, 6.0, 2.5, 21.0)?);
                pb.push_rect(tiny_skia::Rect::from_xywh(13.0, 6.0, 12.0, 3.5)?);
                let path = pb.finish()?;
                pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
            }
        }
        if playing {
            for (i, v) in w.bars.iter().enumerate() {
                let h = (2.0 + v * 26.0).round();
                let r = tiny_skia::Rect::from_xywh(40.0 + i as f32 * 6.5, 34.0 - h, 4.0, h)?;
                pm.fill_rect(r, &paint, Transform::identity(), None);
            }
        }
        ui::ns_image(&pm, (pw, ph))
    });
    WIDGET.with(|slot| {
        if let (Some(w), Some(img)) = (slot.borrow().as_ref(), image)
            && let Some(button) = w.item.button(mtm)
        {
            button.setImage(Some(&img));
        }
    });
}

/// Keeps the title, artwork, buttons and volume in step with what's playing.
fn refresh(force: bool) {
    let n = now();
    let m = &n.media;
    let playing = m.status == crate::player::MediaStatus::Playing;
    let changed = WIDGET.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(w) = slot.as_mut() else { return false };
        let key = (m.title.clone(), n.version, playing);
        let changed = force || w.shown != key;
        if changed {
            w.shown = key;
            w.art = n.artwork.as_ref().and_then(|a| {
                let mut pm = Pixmap::new(a.width, a.height)?;
                for (dst, &c) in pm.pixels_mut().iter_mut().zip(&a.argb) {
                    *dst = tiny_skia::PremultipliedColorU8::from_rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 255)?;
                }
                Some(pm)
            });
        }
        changed
    });
    WIDGET.with(|slot| {
        let slot = slot.borrow();
        let Some(w) = slot.as_ref() else { return };
        let h = &w.header;
        if changed {
            let title =
                if m.has_session() { if m.title.is_empty() { m.source_app.clone() } else { m.title.clone() } } else { "Nothing playing".into() };
            let artist = if m.has_session() {
                [m.artist.as_str(), m.source_app.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ")
            } else {
                "Play something in Spotify, Music or your browser".into()
            };
            ui::set_text(&h.title, &title);
            ui::set_text(&h.artist, &artist);
            h.art.setImage(n.art_image.as_deref().or(ui::symbol("music.note", "No artwork").as_deref()));
            ui::set_symbol(&h.play, if playing { "pause.fill" } else { "play.fill" }, if playing { "Pause" } else { "Play" });
            if let Some(button) = w.item.button(mtm()) {
                let mut text: String = if m.has_session() && !m.title.is_empty() { format!(" {}", m.title) } else { String::new() };
                if text.chars().count() > 28 {
                    text = text.chars().take(27).collect::<String>() + "…";
                }
                button.setTitle(&NSString::from_str(&text));
                button.setToolTip(Some(&NSString::from_str(&if m.has_session() {
                    format!("{title}\n{artist}\nScroll for volume ({}%)", (n.volume * 100.0).round())
                } else {
                    "WebSkin Player".into()
                })));
            }
        }
        h.volume.setDoubleValue(if n.mute { 0.0 } else { n.volume * 100.0 });
    });
    if changed {
        draw_button();
    }
}

pub fn tick() {
    if is_shown() {
        refresh(false);
        if !wants_frame() {
            // Let the bars settle when the music stops.
            let settled = WIDGET.with(|slot| {
                let mut slot = slot.borrow_mut();
                let Some(w) = slot.as_mut() else { return true };
                let busy = w.bars.iter().any(|b| *b > 0.01);
                w.bars.iter_mut().for_each(|b| *b *= 0.8);
                !busy
            });
            if !settled {
                draw_button();
            }
        }
    }
}

/// For the snapshot hook: the menu's header (artwork, song, buttons, volume).
pub fn header_view() -> Option<Retained<NSView>> {
    WIDGET.with(|slot| slot.borrow().as_ref().and_then(|w| w.header.item.view()))
}

/// For the snapshot hook: the menu-bar item's picture, as TIFF.
pub fn button_image() -> Option<Vec<u8>> {
    WIDGET.with(|slot| {
        let slot = slot.borrow();
        let image = slot.as_ref()?.item.button(mtm())?.image()?;
        image.TIFFRepresentation().map(|d| d.to_vec())
    })
}
