//! Headless compatibility pass: `webskin --test-skins <folder|skin> <out>` loads every skin offscreen,
//! runs its scripts, saves a PNG of its Skin Chooser picture and of each window it opens when
//! applied, plus its element tree, and writes report.txt. Each window is checked for faults a user
//! would see, and compared with the skin's own preview screenshot when it has one.

use crate::images::Artwork;
use crate::player::{Command, MediaSnapshot, MediaSource, MediaStatus, NoMedia};
use crate::render::Canvas;
use crate::settings::Settings;
use crate::skin::audit;
use crate::skin::{NullHost, Shared, Skin, ViewId};
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Default, Debug)]
pub struct Summary {
    pub ok: usize,
    pub with_errors: usize,
    pub failed: usize,
    /// Faults found in the saved windows.
    pub faults: usize,
}

fn skin_files(source: &Path) -> Vec<PathBuf> {
    if !source.is_dir() {
        return vec![source.to_path_buf()];
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(source)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wmz") || e.eq_ignore_ascii_case("wms")))
        .collect();
    files.sort();
    files
}

/// A skin's script errors, the size of its preview picture, the faults in its windows and how
/// they compare with its preview screenshot.
type Outcome = Result<(Vec<String>, Option<(u32, u32)>, Vec<String>, Option<String>), String>;

/// Loads a skin's main view offscreen as WMP's Skin Chooser did, without a player object.
fn load_for_chooser(path: &Path) -> Result<(Rc<Shared>, ViewId), String> {
    let shared = Shared::load(path, Box::new(NoMedia), Settings::scratch(), Rc::new(NullHost), true).map_err(|e| e.0)?;
    let v = shared.load_main_view_offscreen();
    Ok((shared, v))
}

/// A window a skin opens when applied, drawn, with the faults found in it.
struct Window {
    id: String,
    canvas: Canvas,
    tree: String,
    faults: Vec<String>,
}

/// A song playing, as skins are used and as their authors took their preview screenshots.
struct Playing;

impl MediaSource for Playing {
    fn snapshot(&self) -> MediaSnapshot {
        MediaSnapshot {
            title: "Glue".into(),
            artist: "Bicep".into(),
            album: "Bicep".into(),
            source_app: "Spotify".into(),
            status: MediaStatus::Playing,
            duration: 269.0,
            can_next: true,
            can_previous: true,
            can_seek: true,
            can_shuffle: true,
            can_repeat: true,
            ..Default::default()
        }
    }
    fn position(&self) -> f64 {
        40.0
    }
    fn artwork(&self) -> Option<Rc<Artwork>> {
        None
    }
    fn command(&self, _: Command) {}
    fn seek(&self, _: f64) {}
    fn set_shuffle(&self, _: bool) {}
    fn set_repeat(&self, _: bool) {}
    fn volume(&self) -> f64 {
        0.5
    }
    fn set_volume(&self, _: f64) {}
    fn mute(&self) -> bool {
        false
    }
    fn set_mute(&self, _: bool) {}
}

/// Draws every window a skin opens when applied, with a song playing (the Skin Chooser picture is
/// often just an image).
/// Returns them with the script errors raised on the way.
fn applied_windows(path: &Path) -> Result<(Vec<String>, Vec<Window>), String> {
    let shared = Shared::load(path, Box::new(Playing), Settings::scratch(), Rc::new(NullHost), false).map_err(|e| e.0)?;
    shared.open_main_view();
    // Eight seconds of timers: some skins open their main window from one (World of Warcraft),
    // and intros play out.
    let start = Instant::now();
    for step in 1..=80 {
        shared.tick(start + Duration::from_millis(step * 100));
    }
    let views: Vec<ViewId> = shared.read(|s| s.open_views().collect());
    let windows = views
        .into_iter()
        .filter_map(|v| {
            let (id, canvas, tree) = shared.read(|s| {
                let id = s.view(v).id.clone().filter(|id| !id.is_empty()).unwrap_or_else(|| format!("view{v}"));
                Some((id, s.render_view(v, 1.0)?, dump_tree(s, v)))
            })?;
            let faults = [audit::key_colour_left(&shared, v, &canvas), audit::controls_out_of_reach(&shared, v, &canvas)].concat();
            Some(Window { id, canvas, tree, faults })
        })
        .collect();
    let errors = shared.errors.borrow().clone();
    Ok((errors, windows))
}

pub fn run(source: &Path, out: &Path) -> std::io::Result<Summary> {
    std::fs::create_dir_all(out)?;
    crate::set_quiet(true);
    let mut report = String::new();
    let mut summary = Summary::default();
    for file in skin_files(source) {
        let name = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let result = catch_unwind(AssertUnwindSafe(|| -> Outcome {
            let (preview, pv) = load_for_chooser(&file)?;
            let size = preview.read(|s| s.render_view(pv, 1.0)).map(|c| save_png(&trim(&c), &out.join(format!("{name}.png"))));
            let screenshot = preview.read(|s| PREVIEWS.iter().find_map(|p| s.images.get_raw(p)));
            let mut faults = vec![];
            let mut best: Option<(audit::PreviewMatch, String)> = None;
            let (errors, windows) = applied_windows(&file)?;
            for w in windows {
                let picture = trim(&w.canvas);
                save_png(&picture, &out.join(format!("{name}.{}.png", w.id)));
                std::fs::write(out.join(format!("{name}.{}.tree.txt", w.id)), &w.tree).map_err(|e| e.to_string())?;
                faults.extend(w.faults.into_iter().map(|f| format!("{}: {f}", w.id)));
                if let Some(m) = screenshot.as_ref().and_then(|p| audit::compare_preview(p, &picture))
                    && best.as_ref().is_none_or(|(b, _)| m.score > b.score)
                {
                    best = Some((m, w.id));
                }
            }
            let compared = best.map(|(m, id)| {
                save_png(&m.picture, &out.join(format!("{name}.preview.png")));
                let side = |n: Option<usize>| n.map_or_else(|| "-".to_string(), |n| n.to_string());
                format!("{:.1}% like its preview ({id}), {} extra, {} missing", m.score * 100.0, side(m.extra), side(m.missing))
            });
            Ok((errors, size.flatten(), faults, compared))
        }));
        match result {
            Ok(Ok((errors, size, faults, compared))) => {
                if errors.is_empty() {
                    summary.ok += 1;
                    let (w, h) = size.unwrap_or_default();
                    let _ = writeln!(report, "OK      {name}  ({w}x{h})");
                } else {
                    summary.with_errors += 1;
                    let _ = writeln!(report, "ERRORS  {name}: {} script error(s)", errors.len());
                    for e in errors.iter().take(8) {
                        let _ = writeln!(report, "          {e}");
                    }
                }
                summary.faults += faults.len();
                for f in &faults {
                    let _ = writeln!(report, "FAULT     {f}");
                }
                if let Some(c) = compared {
                    let _ = writeln!(report, "PREVIEW   {c}");
                }
            }
            Ok(Err(e)) => {
                summary.failed += 1;
                let _ = writeln!(report, "FAILED  {name}: {e}");
            }
            Err(panic) => {
                summary.failed += 1;
                let msg = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                let _ = writeln!(report, "FAILED  {name}: panic: {msg}");
            }
        }
    }
    let _ = writeln!(
        report,
        "\n{} OK, {} with script errors, {} failed, {} faults in windows",
        summary.ok, summary.with_errors, summary.failed, summary.faults
    );
    std::fs::write(out.join("report.txt"), &report)?;
    print!("{report}");
    Ok(summary)
}

/// Where skins keep the screenshot the Skin Chooser shows.
const PREVIEWS: [&str; 4] = ["preview.png", "preview.jpg", "preview.bmp", "preview.gif"];

fn dump_tree(s: &Skin, v: ViewId) -> String {
    let mut out = String::new();
    dump(s, v, 0, 0, &mut out);
    out
}

fn dump(s: &Skin, v: ViewId, e: usize, depth: usize, out: &mut String) {
    let b = s.bounds(v, e);
    let el = s.el(v, e);
    let id = el.id().map(|i| format!(" id={i}")).unwrap_or_default();
    let _ = write!(
        out,
        "{}<{}{id}> abs=({},{},{}x{}) rel=({},{}) z={}",
        "  ".repeat(depth),
        el.tag,
        b.x,
        b.y,
        b.w,
        b.h,
        s.get_int(v, e, "left"),
        s.get_int(v, e, "top"),
        s.get_int(v, e, "zindex")
    );
    if !s.visible(v, e) {
        out.push_str(" HIDDEN");
    }
    for a in ["backgroundimage", "image", "value", "horizontalalignment", "verticalalignment"] {
        if s.has_value(v, e, a) && !s.get_str(v, e, a).is_empty() {
            let _ = write!(out, " {a}={}", s.get_str(v, e, a));
        }
    }
    out.push('\n');
    for &c in &el.children {
        dump(s, v, c, depth + 1, out);
    }
}

/// Crops fully transparent margins (many skins sit in a much larger transparent view).
fn trim(c: &Canvas) -> tiny_skia::Pixmap {
    let pm = &c.pixmap;
    let (w, h) = (pm.width(), pm.height());
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if pm.pixel(x, y).is_some_and(|p| p.alpha() > 8) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return pm.clone();
    }
    tiny_skia::IntRect::from_ltrb(x0 as i32, y0 as i32, x1 as i32, y1 as i32).and_then(|r| pm.clone_rect(r)).unwrap_or_else(|| pm.clone())
}

/// Writes a PNG; returns its size.
fn save_png(pm: &tiny_skia::Pixmap, path: &Path) -> Option<(u32, u32)> {
    let rgba: Vec<u8> = pm
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let img = image::RgbaImage::from_raw(pm.width(), pm.height(), rgba)?;
    img.save_with_format(path, image::ImageFormat::Png).ok()?;
    Some((pm.width(), pm.height()))
}
