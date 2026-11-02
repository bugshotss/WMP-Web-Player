//! Small helpers for building the app's own windows (full mode, the Skin Chooser, the menu-bar player)
//! from AppKit controls: actions as Rust closures, labels, symbol buttons, sliders and pop-ups.

use crate::skin::{AppAction, MenuItem};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezelStyle, NSButton, NSButtonType, NSColor, NSControl, NSDownArrowFunctionKey, NSEventModifierFlags, NSFont,
    NSFontWeightRegular, NSFontWeightSemibold, NSImage, NSImageScaling, NSLineBreakMode, NSMenu, NSMenuItem, NSPopUpButton, NSSlider, NSTextField,
    NSUpArrowFunctionKey, NSView,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGColorSpace, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::rc::Rc;

type Handler = Rc<dyn Fn()>;

thread_local! {
    static HANDLERS: RefCell<Vec<Handler>> = const { RefCell::new(Vec::new()) };
    static TARGET: RefCell<Option<Retained<Actions>>> = const { RefCell::new(None) };
}

define_class!(
    /// The target of every control built here: runs the closure registered under the sender's tag.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinActions"]
    pub struct Actions;

    impl Actions {
        #[unsafe(method(act:))]
        fn act(&self, sender: &AnyObject) {
            let tag: isize = unsafe { msg_send![sender, tag] };
            let handler = HANDLERS.with(|h| usize::try_from(tag - TAG_BASE).ok().and_then(|i| h.borrow().get(i).cloned()));
            if let Some(f) = handler {
                f();
            }
        }
    }
);

/// Tags start here so they never collide with the small tags other code uses.
const TAG_BASE: isize = 100_000;

pub fn target(mtm: MainThreadMarker) -> Retained<Actions> {
    TARGET.with(|t| t.borrow_mut().get_or_insert_with(|| unsafe { msg_send![Actions::alloc(mtm), init] }).clone())
}

fn register(f: impl Fn() + 'static) -> isize {
    HANDLERS.with(|h| {
        let mut h = h.borrow_mut();
        h.push(Rc::new(f));
        TAG_BASE + h.len() as isize - 1
    })
}

/// Runs `f` when the control is used.
pub fn on_action(control: &NSControl, f: impl Fn() + 'static) {
    let mtm = MainThreadMarker::from(control);
    control.setTag(register(f));
    unsafe {
        control.setTarget(Some(&target(mtm)));
        control.setAction(Some(sel!(act:)));
    }
}

/// Runs `f` when the menu item is chosen.
pub fn on_menu(item: &NSMenuItem, f: impl Fn() + 'static) {
    let mtm = MainThreadMarker::from(item);
    item.setTag(register(f));
    unsafe {
        item.setTarget(Some(&target(mtm)));
        item.setAction(Some(sel!(act:)));
    }
}

/// A menu item that runs `f`.
pub fn menu_item(mtm: MainThreadMarker, title: &str, key: &str, f: impl Fn() + 'static) -> Retained<NSMenuItem> {
    let item =
        unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), None, &NSString::from_str(key)) };
    on_menu(&item, f);
    item
}

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

/// Which edges a view keeps its distance from as its parent resizes (in a flipped parent "top" is min Y).
pub struct Pin;

impl Pin {
    pub const FILL: NSAutoresizingMaskOptions =
        NSAutoresizingMaskOptions(NSAutoresizingMaskOptions::ViewWidthSizable.0 | NSAutoresizingMaskOptions::ViewHeightSizable.0);
    pub const WIDTH: NSAutoresizingMaskOptions = NSAutoresizingMaskOptions::ViewWidthSizable;
    /// Stays at the far end on X.
    pub const RIGHT: NSAutoresizingMaskOptions = NSAutoresizingMaskOptions::ViewMinXMargin;
    /// Stays at the far end on Y (the bottom of a flipped view).
    pub const FAR_Y: NSAutoresizingMaskOptions = NSAutoresizingMaskOptions::ViewMinYMargin;
    /// Stays at the near end on Y (the top of a flipped view).
    pub const NEAR_Y: NSAutoresizingMaskOptions = NSAutoresizingMaskOptions::ViewMaxYMargin;
    pub const CENTER_X: NSAutoresizingMaskOptions =
        NSAutoresizingMaskOptions(NSAutoresizingMaskOptions::ViewMinXMargin.0 | NSAutoresizingMaskOptions::ViewMaxXMargin.0);
    pub const CENTER: NSAutoresizingMaskOptions = NSAutoresizingMaskOptions(
        NSAutoresizingMaskOptions::ViewMinXMargin.0
            | NSAutoresizingMaskOptions::ViewMaxXMargin.0
            | NSAutoresizingMaskOptions::ViewMinYMargin.0
            | NSAutoresizingMaskOptions::ViewMaxYMargin.0,
    );

    pub fn and(a: NSAutoresizingMaskOptions, b: NSAutoresizingMaskOptions) -> NSAutoresizingMaskOptions {
        NSAutoresizingMaskOptions(a.0 | b.0)
    }
}

pub fn place(view: &NSView, parent: &NSView, frame: NSRect, pin: NSAutoresizingMaskOptions) {
    view.setFrame(frame);
    view.setAutoresizingMask(pin);
    parent.addSubview(view);
}

pub fn color(rgb: u32, alpha: f64) -> Retained<NSColor> {
    let ch = |s: u32| f64::from((rgb >> s) & 255) / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(ch(16), ch(8), ch(0), alpha)
}

pub fn font(size: f64, bold: bool) -> Retained<NSFont> {
    NSFont::systemFontOfSize_weight(size, unsafe { if bold { NSFontWeightSemibold } else { NSFontWeightRegular } })
}

/// A one-line label that truncates at its end.
pub fn label(mtm: MainThreadMarker, text: &str, size: f64, bold: bool, rgb: u32) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    l.setFont(Some(&font(size, bold)));
    l.setTextColor(Some(&color(rgb, 1.0)));
    l.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    l.setUsesSingleLineMode(true);
    l
}

pub fn set_text(label: &NSTextField, text: &str) {
    if label.stringValue().to_string() != text {
        label.setStringValue(&NSString::from_str(text));
    }
}

/// An SF Symbol, falling back to nothing on systems without it.
pub fn symbol(name: &str, description: &str) -> Option<Retained<NSImage>> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), Some(&NSString::from_str(description)))
}

/// A borderless button showing an SF Symbol, with a tooltip.
pub fn symbol_button(mtm: MainThreadMarker, name: &str, tip: &str, f: impl Fn() + 'static) -> Retained<NSButton> {
    let b = unsafe { NSButton::buttonWithTitle_target_action(&NSString::new(), None, None, mtm) };
    set_symbol(&b, name, tip);
    b.setBordered(false);
    b.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    b.setToolTip(Some(&NSString::from_str(tip)));
    on_action(&b, f);
    b
}

pub fn set_symbol(b: &NSButton, name: &str, tip: &str) {
    if let Some(img) = symbol(name, tip) {
        b.setImage(Some(&img));
    }
    b.setToolTip(Some(&NSString::from_str(tip)));
}

/// A push button with a title.
pub fn text_button(mtm: MainThreadMarker, title: &str, f: impl Fn() + 'static) -> Retained<NSButton> {
    let b = unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str(title), None, None, mtm) };
    b.setBezelStyle(NSBezelStyle::Push);
    on_action(&b, f);
    b
}

/// A button that stays in or out (shuffle, repeat, Always on Top).
pub fn toggle_button(mtm: MainThreadMarker, name: &str, tip: &str, f: impl Fn() + 'static) -> Retained<NSButton> {
    let b = symbol_button(mtm, name, tip, f);
    b.setButtonType(NSButtonType::PushOnPushOff);
    b
}

/// Shows a toggle's state by its colour (the accent colour when on).
pub fn show_toggle(b: &NSButton, on: bool) {
    b.setState(isize::from(on));
    let tint = if on { NSColor::controlAccentColor() } else { color(0xC8D0DA, 1.0) };
    b.setContentTintColor(Some(&tint));
}

pub fn slider(mtm: MainThreadMarker, max: f64, f: impl Fn() + 'static) -> Retained<NSSlider> {
    let s = unsafe { NSSlider::sliderWithValue_minValue_maxValue_target_action(0.0, 0.0, max, None, None, mtm) };
    on_action(&s, f);
    s
}

pub fn popup(mtm: MainThreadMarker, f: impl Fn() + 'static) -> Retained<NSPopUpButton> {
    let p = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), rect(0.0, 0.0, 150.0, 24.0), false);
    on_action(&p, f);
    p
}

/// Replaces a pop-up's items (keeping the selection when it is still there).
pub fn set_items(p: &NSPopUpButton, items: &[String], selected: usize) {
    let same = p.numberOfItems() as usize == items.len() && items.iter().enumerate().all(|(i, t)| p.itemTitleAtIndex(i as isize).to_string() == *t);
    if !same {
        p.removeAllItems();
        for t in items {
            p.addItemWithTitle(&NSString::from_str(t));
        }
    }
    if p.indexOfSelectedItem() != selected as isize {
        p.selectItemAtIndex(selected as isize);
    }
}

/// An image of a premultiplied RGBA pixmap, in `space` (device RGB when None).
pub fn cg_image(pm: &tiny_skia::Pixmap, space: Option<&CGColorSpace>) -> Option<CFRetained<CGImage>> {
    let device = CGColorSpace::new_device_rgb()?;
    let space = space.unwrap_or(&device);
    let info = CGImageByteOrderInfo::Order32Big.0 | CGImageAlphaInfo::PremultipliedLast.0;
    let mut data = pm.data().to_vec();
    let ctx = unsafe {
        objc2_core_graphics::CGBitmapContextCreate(
            data.as_mut_ptr().cast(),
            pm.width() as usize,
            pm.height() as usize,
            8,
            pm.width() as usize * 4,
            Some(space),
            info,
        )
    }?;
    objc2_core_graphics::CGBitmapContextCreateImage(Some(&ctx))
}

/// The CGImage as the object a layer's `contents` takes.
pub fn layer_contents(image: &CFRetained<CGImage>) -> &AnyObject {
    unsafe { &*CFRetained::as_ptr(image).as_ptr().cast::<AnyObject>() }
}

/// An NSImage of a pixmap, `points` wide and tall (it may hold more pixels, for Retina).
pub fn ns_image(pm: &tiny_skia::Pixmap, points: (f64, f64)) -> Option<Retained<NSImage>> {
    let cg = cg_image(pm, None)?;
    Some(NSImage::initWithCGImage_size(NSImage::alloc(), &cg, NSSize::new(points.0, points.1)))
}

/// An NSImage from encoded bytes (a PNG downloaded from archive.org).
pub fn ns_image_from_bytes(bytes: &[u8]) -> Option<Retained<NSImage>> {
    let data = objc2_foundation::NSData::with_bytes(bytes);
    NSImage::initWithData(NSImage::alloc(), &data)
}

// ---- menus built from the engine's menu lists ----------------------------------------------

thread_local! {
    /// The actions of every menu filled by [`fill_menu`], by the menu's address; an item's tag indexes them.
    static MENU_ACTIONS: RefCell<std::collections::HashMap<usize, Vec<AppAction>>> = RefCell::default();
    static RUNNER: RefCell<Option<Retained<MenuRunner>>> = const { RefCell::new(None) };
}

define_class!(
    /// The target of items in menus filled from [`MenuItem`] lists: runs the item's [`AppAction`].
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinMenuRunner"]
    pub struct MenuRunner;

    impl MenuRunner {
        #[unsafe(method(run:))]
        fn run(&self, sender: &NSMenuItem) {
            let Some(menu) = (unsafe { sender.menu() }) else { return };
            let key = Retained::as_ptr(&menu) as usize;
            let action = MENU_ACTIONS.with(|m| m.borrow().get(&key).and_then(|a| usize::try_from(sender.tag()).ok().and_then(|i| a.get(i).cloned())));
            if let Some(action) = action {
                super::run_app_action(&action);
            }
        }
    }
);

fn runner(mtm: MainThreadMarker) -> Retained<MenuRunner> {
    RUNNER.with(|r| r.borrow_mut().get_or_insert_with(|| unsafe { msg_send![MenuRunner::alloc(mtm), init] }).clone())
}

/// Forgets the actions of a menu and its submenus (before it is refilled or dropped).
fn forget(menu: &NSMenu) {
    for item in menu.itemArray().iter() {
        if let Some(sub) = item.submenu() {
            forget(&sub);
        }
    }
    MENU_ACTIONS.with(|m| m.borrow_mut().remove(&(menu as *const NSMenu as usize)));
}

/// Replaces a menu's items with `items`, each running its action when chosen.
pub fn fill_menu(menu: &NSMenu, items: Vec<(MenuItem, AppAction)>) {
    forget(menu);
    menu.removeAllItems();
    menu.setAutoenablesItems(false);
    let mut actions = Vec::new();
    for (item, action) in items {
        add_entry(menu, &item, action, &mut actions);
    }
    MENU_ACTIONS.with(|m| m.borrow_mut().insert(menu as *const NSMenu as usize, actions));
}

/// A new menu filled with `items`.
pub fn new_menu(mtm: MainThreadMarker, title: &str, items: Vec<(MenuItem, AppAction)>) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    fill_menu(&menu, items);
    menu
}

fn add_entry(menu: &NSMenu, item: &MenuItem, action: AppAction, actions: &mut Vec<AppAction>) {
    let mtm = MainThreadMarker::from(menu);
    if item.separator {
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        return;
    }
    let mi =
        unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(&item.label), None, &NSString::new()) };
    apply_shortcut(&mi, &item.shortcut);
    mi.setEnabled(item.enabled);
    mi.setState(isize::from(item.checked));
    match action {
        AppAction::Submenu(sub_actions) => {
            let sub = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(&item.label));
            fill_menu(&sub, item.submenu.iter().cloned().zip(sub_actions).collect());
            mi.setSubmenu(Some(&sub));
        }
        a => {
            unsafe {
                mi.setTarget(Some(&runner(mtm)));
                mi.setAction(Some(sel!(run:)));
            }
            mi.setTag(actions.len() as isize);
            actions.push(a);
        }
    }
    menu.addItem(&mi);
}

/// Sets a key equivalent written like "cmd+p", "shift+cmd+h", "opt+cmd+down" or "ctrl+cmd+f".
pub fn apply_shortcut(mi: &NSMenuItem, shortcut: &str) {
    if shortcut.is_empty() {
        return;
    }
    let mut mask = NSEventModifierFlags::empty();
    let mut key = String::new();
    for part in shortcut.split('+') {
        match part.trim().to_lowercase().as_str() {
            "cmd" => mask |= NSEventModifierFlags::Command,
            "shift" => mask |= NSEventModifierFlags::Shift,
            "opt" | "alt" => mask |= NSEventModifierFlags::Option,
            "ctrl" => mask |= NSEventModifierFlags::Control,
            "up" => key = char::from_u32(NSUpArrowFunctionKey).map(String::from).unwrap_or_default(),
            "down" => key = char::from_u32(NSDownArrowFunctionKey).map(String::from).unwrap_or_default(),
            "space" => key = " ".into(),
            other => key = other.to_string(),
        }
    }
    mi.setKeyEquivalent(&NSString::from_str(&key));
    mi.setKeyEquivalentModifierMask(mask);
}

/// Shows a menu of `items` in `view`, at `at` (view coordinates).
pub fn popup_menu(view: &NSView, at: NSPoint, items: Vec<(MenuItem, AppAction)>) {
    let menu = new_menu(MainThreadMarker::from(view), "", items);
    menu.popUpMenuPositioningItem_atLocation_inView(None, at, Some(view));
    forget(&menu);
}

define_class!(
    /// A plain view laid out top-down (y grows downwards), for the app's own windows.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinFlippedView"]
    pub struct Flipped;

    impl Flipped {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

/// A flipped view, filled with `rgb` (None: clear).
pub fn flipped(mtm: MainThreadMarker, frame: NSRect, rgb: Option<u32>) -> Retained<Flipped> {
    let v: Retained<Flipped> = unsafe { msg_send![Flipped::alloc(mtm), initWithFrame: frame] };
    v.setWantsLayer(true);
    if let (Some(rgb), Some(layer)) = (rgb, v.layer()) {
        let ch = |s: u32| f64::from((rgb >> s) & 255) / 255.0;
        layer.setBackgroundColor(Some(&objc2_core_graphics::CGColor::new_srgb(ch(16), ch(8), ch(0), 1.0)));
    }
    v
}
