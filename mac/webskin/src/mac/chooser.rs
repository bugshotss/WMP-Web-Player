//! The Skin Chooser, in full mode's second tab, as in the Windows app: installed skins grouped by publisher
//! with a live preview of each (drawn by the skin engine as WMP's chooser showed it), search and a
//! publisher filter, Apply Skin; and "Get more skins": the 175-skin collection on archive.org with WMP's
//! own screenshot of each, Download and Apply, or Download All.

use super::push;
use super::ui::{self, Flipped, Pin, rect};
use crate::catalog;
use crate::net::{Download, Progress};
use crate::online::{self, OnlineSkin};
use crate::preview::{Previews, SkinInfo};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSButton, NSControlTextEditingDelegate, NSImage, NSImageScaling, NSImageView, NSPopUpButton, NSProgressIndicator, NSScrollView, NSSearchField,
    NSSegmentSwitchTracking, NSSegmentedControl, NSTableColumn, NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextField,
};
use objc2_foundation::{NSArray, NSIndexSet, NSInteger, NSNotification, NSObject, NSObjectProtocol, NSString};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TEXT: u32 = 0xE6EAF0;
const DIM: u32 = 0x8B95A3;
const ALL_PUBLISHERS: &str = "All publishers";

/// A skin in one of the two lists.
#[derive(Clone, Debug)]
struct Entry {
    /// The file name, lower case: how the two lists recognise the same skin.
    key: String,
    file: String,
    path: Option<PathBuf>,
    name: String,
    publisher: String,
    designer: Option<String>,
    online: Option<OnlineSkin>,
    installed: bool,
}

#[derive(Clone, Debug)]
enum Row {
    /// A publisher's heading.
    Group,
    Skin(usize),
}

type Listing = Arc<Mutex<Option<Result<Vec<OnlineSkin>, String>>>>;
type Shots = Arc<Mutex<Vec<(String, Option<Vec<u8>>)>>>;

#[derive(Default)]
struct Downloads {
    queue: VecDeque<OnlineSkin>,
    current: Option<(OnlineSkin, Download)>,
    apply_after: bool,
    done_bytes: u64,
    total_bytes: u64,
    done: usize,
    total: usize,
    failed: Vec<String>,
}

struct State {
    online: bool,
    installed: Vec<Entry>,
    online_list: Vec<OnlineSkin>,
    listing: Option<Listing>,
    online_error: Option<String>,
    rows: Vec<Row>,
    selected: Option<String>,
    previews: Previews,
    images: HashMap<String, Option<Retained<NSImage>>>,
    shots: Shots,
    shots_requested: HashSet<String>,
    meta: HashMap<PathBuf, SkinInfo>,
    meta_rx: Option<Receiver<(PathBuf, Option<SkinInfo>)>>,
    downloads: Downloads,
}

pub struct Chooser {
    pub view: Retained<Flipped>,
    source: Retained<NSSegmentedControl>,
    search: Retained<NSSearchField>,
    publisher: Retained<NSPopUpButton>,
    table: Retained<NSTableView>,
    _data: Retained<TableData>,
    count: Retained<NSTextField>,
    name: Retained<NSTextField>,
    author: Retained<NSTextField>,
    preview: Retained<NSImageView>,
    status: Retained<NSTextField>,
    installed_row: Retained<Flipped>,
    online_row: Retained<Flipped>,
    apply: Retained<NSButton>,
    download: Retained<NSButton>,
    download_all: Retained<NSButton>,
    cancel: Retained<NSButton>,
    progress: Retained<NSProgressIndicator>,
    progress_label: Retained<NSTextField>,
    credit: Retained<NSTextField>,
    state: RefCell<State>,
}

thread_local! {
    static CHOOSER: RefCell<Option<Rc<Chooser>>> = const { RefCell::new(None) };
    /// What the table shows: each row's text, and whether it heads a publisher's group.
    static ROWS: RefCell<Vec<(String, bool)>> = const { RefCell::new(Vec::new()) };
}

fn get() -> Option<Rc<Chooser>> {
    CHOOSER.with(|c| c.borrow().clone())
}

define_class!(
    /// The skin list's data source and delegate.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinChooserTable"]
    struct TableData;

    unsafe impl NSObjectProtocol for TableData {}

    unsafe impl NSTableViewDataSource for TableData {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _table: &NSTableView) -> NSInteger {
            ROWS.with(|r| r.borrow().len() as NSInteger)
        }

        #[unsafe(method(tableView:objectValueForTableColumn:row:))]
        fn value(&self, _table: &NSTableView, _column: Option<&NSTableColumn>, row: NSInteger) -> *mut AnyObject {
            let text = ROWS.with(|r| r.borrow().get(row as usize).map(|(t, _)| t.clone())).unwrap_or_default();
            Retained::autorelease_return(Retained::into_super(Retained::into_super(NSString::from_str(&text))))
        }
    }

    unsafe impl NSControlTextEditingDelegate for TableData {}

    unsafe impl NSTableViewDelegate for TableData {
        #[unsafe(method(tableView:isGroupRow:))]
        fn is_group(&self, _table: &NSTableView, row: NSInteger) -> bool {
            ROWS.with(|r| r.borrow().get(row as usize).is_some_and(|(_, g)| *g))
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn should_select(&self, _table: &NSTableView, row: NSInteger) -> bool {
            ROWS.with(|r| r.borrow().get(row as usize).is_some_and(|(_, g)| !*g))
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_changed(&self, _n: &NSNotification) {
            if let Some(c) = get() {
                c.selection_changed();
            }
        }
    }
);

fn installed_entry(path: PathBuf, info: Option<&SkinInfo>) -> Entry {
    let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let (author, copyright, title) = info.map_or(("", "", ""), |i| (i.author.as_str(), i.copyright.as_str(), i.title.as_str()));
    let (publisher, designer) = catalog::lookup(&file, author, copyright);
    let name = if title.is_empty() { catalog::pretty_name(&file) } else { title.to_string() };
    Entry { key: file.to_lowercase(), file, path: Some(path), name, publisher, designer, online: None, installed: true }
}

fn online_entry(skin: &OnlineSkin, installed: &HashSet<String>) -> Entry {
    let (publisher, designer) = catalog::lookup(&skin.file, "", "");
    let key = skin.file.to_lowercase();
    Entry {
        installed: installed.contains(&key),
        key,
        file: skin.file.clone(),
        path: None,
        name: catalog::pretty_name(&skin.file),
        publisher,
        designer,
        online: Some(skin.clone()),
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

impl Chooser {
    pub fn new(mtm: MainThreadMarker, frame: objc2_foundation::NSRect) -> Rc<Chooser> {
        let (w, h) = (frame.size.width, frame.size.height);
        let view = ui::flipped(mtm, frame, Some(0x1E2329));
        let left = ui::flipped(mtm, rect(0.0, 0.0, 280.0, h), Some(0x252B33));
        ui::place(&left, &view, rect(0.0, 0.0, 280.0, h), Pin::and(Pin::NEAR_Y, NSHEIGHT));
        let source = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&[NSString::from_str("Installed"), NSString::from_str("Get more skins")]),
                NSSegmentSwitchTracking::SelectOne,
                None,
                None,
                mtm,
            )
        };
        source.setSelectedSegment(0);
        ui::place(&source, &left, rect(8.0, 8.0, 264.0, 24.0), Pin::NEAR_Y);
        ui::on_action(&source, || {
            if let Some(c) = get() {
                c.set_online(c.source.selectedSegment() == 1);
            }
        });
        let search: Retained<NSSearchField> = unsafe { msg_send![NSSearchField::alloc(mtm), initWithFrame: rect(8.0, 40.0, 264.0, 24.0)] };
        search.setPlaceholderString(Some(&NSString::from_str("Search skins, publishers, designers")));
        ui::place(&search, &left, rect(8.0, 40.0, 264.0, 24.0), Pin::NEAR_Y);
        ui::on_action(&search, || {
            if let Some(c) = get() {
                c.refresh_rows();
            }
        });
        let publisher = ui::popup(mtm, || {
            if let Some(c) = get() {
                c.refresh_rows();
            }
        });
        publisher.setToolTip(Some(&NSString::from_str("Show one publisher's skins")));
        ui::place(&publisher, &left, rect(8.0, 70.0, 264.0, 26.0), Pin::NEAR_Y);

        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(0.0, 102.0, 280.0, h - 102.0 - 26.0));
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        let table = NSTableView::initWithFrame(NSTableView::alloc(mtm), rect(0.0, 0.0, 280.0, h - 128.0));
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &NSString::from_str("skin"));
        column.setWidth(262.0);
        column.setEditable(false);
        table.addTableColumn(&column);
        table.setHeaderView(None);
        table.setRowHeight(20.0);
        table.setBackgroundColor(&objc2_app_kit::NSColor::clearColor());
        table.setColumnAutoresizingStyle(objc2_app_kit::NSTableViewColumnAutoresizingStyle::FirstColumnOnlyAutoresizingStyle);
        let data: Retained<TableData> = unsafe { msg_send![TableData::alloc(mtm), init] };
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*data)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*data)));
        }
        // A double-click applies (or downloads and applies) the skin.
        ui::on_action(&table, || {
            if let Some(c) = get()
                && c.table.clickedRow() >= 0
            {
                c.apply_selected();
            }
        });
        unsafe {
            table.setDoubleAction(Some(sel!(act:)));
            table.setAction(None);
        }
        scroll.setDocumentView(Some(&table));
        ui::place(&scroll, &left, rect(0.0, 102.0, 280.0, h - 128.0), Pin::and(Pin::WIDTH, NSHEIGHT));
        let count = ui::label(mtm, "", 11.0, false, DIM);
        ui::place(&count, &left, rect(10.0, h - 22.0, 260.0, 16.0), Pin::FAR_Y);

        // The skin: its name, who made it, its picture, and what can be done with it.
        let right = ui::flipped(mtm, rect(280.0, 0.0, w - 280.0, h), None);
        ui::place(&right, &view, rect(280.0, 0.0, w - 280.0, h), Pin::FILL);
        let rw = w - 280.0;
        let name = ui::label(mtm, "", 22.0, true, TEXT);
        ui::place(&name, &right, rect(18.0, 14.0, rw - 36.0, 30.0), Pin::and(Pin::WIDTH, Pin::NEAR_Y));
        let author = ui::label(mtm, "", 12.0, false, DIM);
        ui::place(&author, &right, rect(18.0, 44.0, rw - 36.0, 18.0), Pin::and(Pin::WIDTH, Pin::NEAR_Y));
        let frame_view = ui::flipped(mtm, rect(18.0, 72.0, rw - 36.0, h - 72.0 - 120.0), Some(0x0C0E11));
        if let Some(layer) = frame_view.layer() {
            layer.setCornerRadius(6.0);
        }
        ui::place(&frame_view, &right, rect(18.0, 72.0, rw - 36.0, h - 192.0), Pin::FILL);
        let preview = NSImageView::initWithFrame(NSImageView::alloc(mtm), rect(12.0, 12.0, rw - 60.0, h - 216.0));
        preview.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        ui::place(&preview, &frame_view, rect(12.0, 12.0, rw - 60.0, h - 216.0), Pin::FILL);
        let status = ui::label(mtm, "", 13.0, false, DIM);
        status.setAlignment(objc2_app_kit::NSTextAlignment::Center);
        ui::place(&status, &frame_view, rect(12.0, (h - 192.0) / 2.0 - 10.0, rw - 60.0, 20.0), Pin::and(Pin::WIDTH, Pin::CENTER));

        let installed_row = ui::flipped(mtm, rect(18.0, h - 108.0, rw - 36.0, 32.0), None);
        ui::place(&installed_row, &right, rect(18.0, h - 108.0, rw - 36.0, 32.0), Pin::and(Pin::WIDTH, Pin::FAR_Y));
        let apply = ui::text_button(mtm, "Apply Skin", || {
            if let Some(c) = get() {
                c.apply_selected();
            }
        });
        apply.setKeyEquivalent(&NSString::from_str("\r"));
        let add = ui::text_button(mtm, "Add Skins…", || super::push_command("addskins"));
        let folder = ui::text_button(mtm, "Show Skins Folder", || super::push_command("skinsfolder"));
        let mut x = 0.0;
        for (b, bw) in [(&apply, 120.0), (&add, 120.0), (&folder, 160.0)] {
            ui::place(b, &installed_row, rect(x, 0.0, bw, 32.0), Pin::NEAR_Y);
            x += bw + 8.0;
        }
        let online_row = ui::flipped(mtm, rect(18.0, h - 108.0, rw - 36.0, 32.0), None);
        ui::place(&online_row, &right, rect(18.0, h - 108.0, rw - 36.0, 32.0), Pin::and(Pin::WIDTH, Pin::FAR_Y));
        let download = ui::text_button(mtm, "Download and Apply", || {
            if let Some(c) = get() {
                c.apply_selected();
            }
        });
        let download_all = ui::text_button(mtm, "Download All", || {
            if let Some(c) = get() {
                c.download_all();
            }
        });
        let cancel = ui::text_button(mtm, "Cancel", || {
            if let Some(c) = get() {
                c.cancel_downloads();
            }
        });
        let mut x = 0.0;
        for (b, bw) in [(&download, 170.0), (&download_all, 130.0), (&cancel, 90.0)] {
            ui::place(b, &online_row, rect(x, 0.0, bw, 32.0), Pin::NEAR_Y);
            x += bw + 8.0;
        }
        cancel.setHidden(true);
        online_row.setHidden(true);
        let progress = NSProgressIndicator::initWithFrame(NSProgressIndicator::alloc(mtm), rect(18.0, h - 68.0, rw - 236.0, 12.0));
        progress.setIndeterminate(false);
        progress.setMinValue(0.0);
        progress.setMaxValue(1.0);
        progress.setHidden(true);
        ui::place(&progress, &right, rect(18.0, h - 66.0, rw - 236.0, 12.0), Pin::and(Pin::WIDTH, Pin::FAR_Y));
        let progress_label = ui::label(mtm, "", 11.0, false, DIM);
        progress_label.setAlignment(objc2_app_kit::NSTextAlignment::Right);
        ui::place(&progress_label, &right, rect(rw - 210.0, h - 68.0, 192.0, 16.0), Pin::and(Pin::RIGHT, Pin::FAR_Y));
        let credit = NSTextField::wrappingLabelWithString(
            &NSString::from_str(
                "Skins and screenshots come from the Windows Media Player Skins Collection on archive.org. Each skin belongs to its original author.",
            ),
            mtm,
        );
        credit.setFont(Some(&ui::font(11.0, false)));
        credit.setTextColor(Some(&ui::color(DIM, 1.0)));
        credit.setHidden(true);
        ui::place(&credit, &right, rect(18.0, h - 46.0, rw - 36.0, 32.0), Pin::and(Pin::WIDTH, Pin::FAR_Y));

        let state = State {
            online: false,
            installed: vec![],
            online_list: vec![],
            listing: None,
            online_error: None,
            rows: vec![],
            selected: None,
            previews: Previews::new(),
            images: HashMap::new(),
            shots: Arc::default(),
            shots_requested: HashSet::new(),
            meta: HashMap::new(),
            meta_rx: None,
            downloads: Downloads::default(),
        };
        let c = Rc::new(Chooser {
            view,
            source,
            search,
            publisher,
            table,
            _data: data,
            count,
            name,
            author,
            preview,
            status,
            installed_row,
            online_row,
            apply,
            download,
            download_all,
            cancel,
            progress,
            progress_label,
            credit,
            state: RefCell::new(state),
        });
        CHOOSER.with(|slot| *slot.borrow_mut() = Some(c.clone()));
        c.library_changed();
        c
    }

    /// The tab was opened (`online`: on Get more skins).
    pub fn shown(&self, online: bool) {
        self.source.setSelectedSegment(isize::from(online));
        self.set_online(online);
    }

    fn set_online(&self, online: bool) {
        self.state.borrow_mut().online = online;
        self.installed_row.setHidden(online);
        self.online_row.setHidden(!online);
        self.credit.setHidden(!online);
        if online {
            self.fetch_listing();
        }
        self.refresh_rows();
    }

    fn fetch_listing(&self) {
        let mut s = self.state.borrow_mut();
        if !s.online_list.is_empty() || s.listing.is_some() {
            return;
        }
        s.online_error = None;
        let listing: Listing = Arc::default();
        let out = listing.clone();
        std::thread::spawn(move || {
            let result = online::list();
            *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        });
        s.listing = Some(listing);
    }

    /// Skins were added, downloaded or removed: reread the library.
    pub fn library_changed(&self) {
        let paths = crate::library::list(&crate::library::folder());
        let todo: Vec<PathBuf> = {
            let mut s = self.state.borrow_mut();
            let meta = s.meta.clone();
            s.installed = paths.iter().map(|p| installed_entry(p.clone(), meta.get(p))).collect();
            paths.into_iter().filter(|p| !meta.contains_key(p)).collect()
        };
        // Titles, authors and copyrights come from each skin's .wms, read off the main thread.
        if !todo.is_empty() {
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                for p in todo {
                    let info = crate::preview::info(&p);
                    if tx.send((p, info)).is_err() {
                        break;
                    }
                }
            });
            self.state.borrow_mut().meta_rx = Some(rx);
        }
        self.refresh_rows();
    }

    fn entries(&self) -> Vec<Entry> {
        let s = self.state.borrow();
        if s.online {
            let installed: HashSet<String> = s.installed.iter().map(|e| e.key.clone()).collect();
            s.online_list.iter().map(|o| online_entry(o, &installed)).collect()
        } else {
            s.installed.clone()
        }
    }

    /// Filters by the search text and publisher, groups by publisher, and refills the list.
    fn refresh_rows(&self) {
        let all = self.entries();
        let query = self.search.stringValue().to_string().to_lowercase();
        // The publisher filter lists every publisher in this list, with how many skins each has.
        let mut counts: HashMap<String, usize> = HashMap::new();
        for e in &all {
            *counts.entry(e.publisher.clone()).or_default() += 1;
        }
        let mut publishers: Vec<String> = counts.keys().cloned().collect();
        publishers.sort_by_key(|p| catalog::sort_key(p));
        let chosen = self.publisher.titleOfSelectedItem().map(|t| t.to_string()).unwrap_or_default();
        let chosen = chosen.rsplit_once("  (").map_or(chosen.clone(), |(p, _)| p.to_string());
        let titles: Vec<String> =
            std::iter::once(format!("{ALL_PUBLISHERS}  ({})", all.len())).chain(publishers.iter().map(|p| format!("{p}  ({})", counts[p]))).collect();
        let index = publishers.iter().position(|p| *p == chosen).map_or(0, |i| i + 1);
        ui::set_items(&self.publisher, &titles, index);
        let only = (index > 0).then(|| publishers[index - 1].clone());

        let mut shown: Vec<(usize, &Entry)> = all
            .iter()
            .enumerate()
            .filter(|(_, e)| only.as_ref().is_none_or(|p| e.publisher == *p))
            .filter(|(_, e)| {
                query.is_empty()
                    || e.name.to_lowercase().contains(&query)
                    || e.publisher.to_lowercase().contains(&query)
                    || e.designer.as_ref().is_some_and(|d| d.to_lowercase().contains(&query))
            })
            .collect();
        shown.sort_by_key(|(_, e)| (catalog::sort_key(&e.publisher), e.name.to_lowercase()));
        let mut rows = Vec::new();
        let mut table_rows = Vec::new();
        let mut i = 0;
        while i < shown.len() {
            let publisher = shown[i].1.publisher.clone();
            let n = shown[i..].iter().take_while(|(_, e)| e.publisher == publisher).count();
            rows.push(Row::Group);
            table_rows.push((format!("{publisher}  {n}"), true));
            for (index, e) in &shown[i..i + n] {
                rows.push(Row::Skin(*index));
                let mark = if self.state.borrow().online && e.installed { "   ✓" } else { "" };
                table_rows.push((format!("{}{mark}", e.name), false));
            }
            i += n;
        }
        let (online, loading, error, selected) = {
            let s = self.state.borrow();
            (s.online, s.listing.is_some(), s.online_error.clone(), s.selected.clone())
        };
        let count_text = match (online, loading, &error) {
            (true, true, _) => "Loading the archive.org collection…".to_string(),
            (true, false, Some(e)) => format!("archive.org can't be reached: {e}"),
            (true, false, None) => format!("{} skins on archive.org · {} installed", all.len(), all.iter().filter(|e| e.installed).count()),
            (false, _, _) => format!("{} skins installed", all.len()),
        };
        ui::set_text(&self.count, &count_text);
        self.state.borrow_mut().rows = rows;
        ROWS.with(|r| *r.borrow_mut() = table_rows);
        self.table.reloadData();
        // Keep the selection on the same skin, or pick the first.
        let entries = all;
        let row = {
            let s = self.state.borrow();
            let find = |key: &str| s.rows.iter().position(|r| matches!(r, Row::Skin(i) if entries[*i].key == key));
            selected.as_deref().and_then(find).or_else(|| s.rows.iter().position(|r| matches!(r, Row::Skin(_))))
        };
        match row {
            Some(r) => {
                self.table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(r), false);
                self.table.scrollRowToVisible(r as isize);
            }
            None => unsafe { self.table.deselectAll(None) },
        }
        self.selection_changed();
    }

    fn selected_entry(&self) -> Option<Entry> {
        let row = self.table.selectedRow();
        let entries = self.entries();
        let s = self.state.borrow();
        match s.rows.get(usize::try_from(row).ok()?)? {
            Row::Skin(i) => entries.get(*i).cloned(),
            Row::Group => None,
        }
    }

    fn selection_changed(&self) {
        let entry = self.selected_entry();
        self.state.borrow_mut().selected = entry.as_ref().map(|e| e.key.clone());
        let Some(e) = entry else {
            ui::set_text(&self.name, "");
            ui::set_text(&self.author, "");
            self.preview.setImage(None);
            let online = self.state.borrow().online;
            ui::set_text(&self.status, if online { "" } else { "No skins yet: open Get more skins, or Add Skins…" });
            self.apply.setEnabled(false);
            self.download.setEnabled(false);
            return;
        };
        ui::set_text(&self.name, &e.name);
        let who = match &e.designer {
            Some(d) if *d != e.publisher => format!("{} · designed by {d}", e.publisher),
            _ => e.publisher.clone(),
        };
        let size = e.online.as_ref().map(|o| format!(" · {}", megabytes(o.size))).unwrap_or_default();
        let installed = if e.online.is_some() && e.installed { " · installed" } else { "" };
        ui::set_text(&self.author, &format!("{who}{size}{installed}"));
        self.apply.setEnabled(true);
        self.download.setEnabled(true);
        self.download.setTitle(&NSString::from_str(if e.installed { "Apply Skin" } else { "Download and Apply" }));
        self.show_preview(&e);
    }

    fn show_preview(&self, e: &Entry) {
        let cached = self.state.borrow().images.get(&preview_key(e)).cloned();
        match cached {
            Some(Some(img)) => {
                self.preview.setImage(Some(&img));
                ui::set_text(&self.status, "");
            }
            Some(None) => {
                self.preview.setImage(None);
                ui::set_text(&self.status, "No preview");
            }
            None => {
                self.preview.setImage(None);
                ui::set_text(&self.status, "Loading preview…");
                let mut s = self.state.borrow_mut();
                match (&e.path, &e.online) {
                    (Some(path), _) => s.previews.request(path, 2.0),
                    (None, Some(o)) if s.shots_requested.insert(e.key.clone()) => {
                        let (url, key, out) = (o.preview_url(), e.key.clone(), s.shots.clone());
                        std::thread::spawn(move || {
                            let bytes = crate::net::get(&url, &[], Duration::from_secs(30)).ok();
                            out.lock().unwrap_or_else(|e| e.into_inner()).push((key, bytes));
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    fn apply_selected(&self) {
        let Some(e) = self.selected_entry() else { return };
        if let Some(path) = e.path.clone().or_else(|| e.installed.then(|| crate::library::folder().join(&e.file))) {
            push(super::Pending::Load(path));
            return;
        }
        if let Some(o) = e.online {
            self.queue(vec![o], true);
        }
    }

    fn download_all(&self) {
        let todo: Vec<OnlineSkin> = self.entries().into_iter().filter(|e| !e.installed).filter_map(|e| e.online).collect();
        if todo.is_empty() {
            super::message_box("Every skin is installed", "All the skins in the archive.org collection are in your Skins menu already.");
            return;
        }
        self.queue(todo, false);
    }

    fn queue(&self, skins: Vec<OnlineSkin>, apply_after: bool) {
        let mut s = self.state.borrow_mut();
        let d = &mut s.downloads;
        if d.current.is_none() && d.queue.is_empty() {
            *d = Downloads::default();
        }
        d.apply_after = apply_after && skins.len() == 1;
        d.total += skins.len();
        d.total_bytes += skins.iter().map(|s| s.size).sum::<u64>();
        d.queue.extend(skins);
    }

    fn cancel_downloads(&self) {
        let mut s = self.state.borrow_mut();
        s.downloads.queue.clear();
        if let Some((_, d)) = &s.downloads.current {
            d.cancel();
        }
    }

    /// Previews, metadata, the archive.org listing and downloads as they arrive.
    pub fn tick(&self) {
        self.poll_listing();
        self.poll_meta();
        self.poll_previews();
        self.poll_downloads();
    }

    fn poll_listing(&self) {
        let done = {
            let s = self.state.borrow();
            s.listing.as_ref().and_then(|l| l.lock().unwrap_or_else(|e| e.into_inner()).take())
        };
        let Some(result) = done else { return };
        {
            let mut s = self.state.borrow_mut();
            s.listing = None;
            match result {
                Ok(list) => s.online_list = list,
                Err(e) => {
                    crate::log(&format!("archive.org listing failed: {e}"));
                    s.online_error = Some(e);
                }
            }
        }
        self.refresh_rows();
    }

    fn poll_meta(&self) {
        let mut got = vec![];
        {
            let s = self.state.borrow();
            if let Some(rx) = &s.meta_rx {
                while let Ok(m) = rx.try_recv() {
                    got.push(m);
                }
            }
        }
        if got.is_empty() {
            return;
        }
        {
            let mut s = self.state.borrow_mut();
            for (path, info) in got {
                s.meta.insert(path, info.unwrap_or_default());
            }
            let meta = s.meta.clone();
            for e in &mut s.installed {
                if let Some(path) = e.path.clone() {
                    *e = installed_entry(path.clone(), meta.get(&path));
                }
            }
        }
        // Only regroup once all of them are in, so the list doesn't jump about.
        let pending = self.state.borrow().installed.iter().any(|e| e.path.as_ref().is_some_and(|p| !self.state.borrow().meta.contains_key(p)));
        if !pending && !self.state.borrow().online {
            self.refresh_rows();
        }
    }

    fn poll_previews(&self) {
        let mut changed = false;
        loop {
            let done = self.state.borrow().previews.poll();
            let Some(done) = done else { break };
            let key = done.path.file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
            let image = match done.image {
                Ok(pm) => ui::ns_image(&pm, (f64::from(pm.width()) / 2.0, f64::from(pm.height()) / 2.0)),
                Err(e) => {
                    crate::log(&format!("Preview of {} failed: {e}", done.path.display()));
                    None
                }
            };
            self.state.borrow_mut().images.insert(format!("i:{key}"), image);
            changed = true;
        }
        let shots: Vec<(String, Option<Vec<u8>>)> = {
            let s = self.state.borrow();
            std::mem::take(&mut *s.shots.lock().unwrap_or_else(|e| e.into_inner()))
        };
        for (key, bytes) in shots {
            let image = bytes.as_deref().and_then(ui::ns_image_from_bytes);
            self.state.borrow_mut().images.insert(format!("o:{key}"), image);
            changed = true;
        }
        if changed && let Some(e) = self.selected_entry() {
            let cached = self.state.borrow().images.contains_key(&preview_key(&e));
            if cached {
                self.show_preview(&e);
            }
        }
    }

    fn poll_downloads(&self) {
        // Start the next one.
        {
            let mut s = self.state.borrow_mut();
            let folder = crate::library::folder();
            if s.downloads.current.is_none()
                && let Some(next) = s.downloads.queue.pop_front()
            {
                let d = Download::start(&next.url(), &folder.join(&next.file), next.size, crate::net::check_zip);
                s.downloads.current = Some((next, d));
            }
        }
        let (busy, fraction, label, finished) = {
            let mut s = self.state.borrow_mut();
            let d = &mut s.downloads;
            let mut finished = None;
            let mut current_bytes = 0;
            if let Some((skin, dl)) = &d.current {
                match dl.progress() {
                    Progress::Running { bytes } => current_bytes = bytes,
                    Progress::Done(result) => finished = Some((skin.clone(), result)),
                }
            }
            if let Some((skin, result)) = &finished {
                d.current = None;
                d.done += 1;
                d.done_bytes += skin.size;
                if let Err(e) = result {
                    d.failed.push(format!("{}: {e}", skin.file));
                }
            }
            let busy = d.current.is_some() || !d.queue.is_empty();
            let fraction = if d.total_bytes > 0 { (d.done_bytes + current_bytes) as f64 / d.total_bytes as f64 } else { 0.0 };
            let label = if d.total > 1 {
                format!("{} of {} · {} of {}", (d.done + 1).min(d.total), d.total, megabytes(d.done_bytes + current_bytes), megabytes(d.total_bytes))
            } else {
                format!("{} of {}", megabytes(current_bytes), megabytes(d.total_bytes))
            };
            (busy, fraction.min(1.0), label, finished)
        };
        self.progress.setHidden(!busy);
        self.progress_label.setHidden(!busy);
        self.cancel.setHidden(!busy);
        self.download_all.setEnabled(!busy);
        if busy {
            self.progress.setDoubleValue(fraction);
            ui::set_text(&self.progress_label, &label);
        }
        let Some((skin, result)) = finished else { return };
        let apply = {
            let s = self.state.borrow();
            s.downloads.apply_after
        };
        match result {
            Ok(path) => {
                crate::log(&format!("Downloaded {}", skin.file));
                if !busy {
                    super::library_changed();
                }
                if apply && !busy {
                    push(super::Pending::Load(path));
                }
            }
            Err(e) if e == "cancelled" => super::library_changed(),
            Err(e) => {
                crate::log(&format!("Download of {} failed: {e}", skin.file));
                if !busy {
                    super::library_changed();
                }
            }
        }
        if !busy {
            let failed = std::mem::take(&mut self.state.borrow_mut().downloads.failed);
            if !failed.is_empty() {
                super::message_box("Some skins didn't download", &failed.join("\n"));
            }
        }
    }
}

fn preview_key(e: &Entry) -> String {
    if e.path.is_some() { format!("i:{}", e.key) } else { format!("o:{}", e.key) }
}

/// Keeps a view's height following its parent's.
const NSHEIGHT: objc2_app_kit::NSAutoresizingMaskOptions = objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable;

/// For the snapshot hook: the selected skin's preview has arrived.
pub fn preview_ready() -> bool {
    get().is_some_and(|c| c.preview.image().is_some())
}
