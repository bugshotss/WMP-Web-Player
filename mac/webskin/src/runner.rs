//! Headless compatibility pass: `webskin --test-skins <folder|skin> <out>` loads every skin offscreen,
//! runs its scripts, saves a PNG of its Skin Chooser picture and of each window it opens when
//! applied, plus its element tree, and writes report.txt.

use crate::player::NoMedia;
use crate::render::Canvas;
use crate::settings::Settings;
use crate::skin::{NullHost, Shared, Skin, ViewId};
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

#[derive(Default, Debug)]
pub struct Summary {
    pub ok: usize,
    pub with_errors: usize,
    pub failed: usize,
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

/// Loads a skin's main view offscreen. `chooser` runs it as WMP's Skin Chooser did, without a
/// player object; otherwise as when the skin is applied.
/// A skin's script errors and the size of its preview picture.
type Outcome = Result<(Vec<String>, Option<(u32, u32)>), String>;

pub(crate) fn load_offscreen(path: &Path, chooser: bool) -> Result<(Rc<Shared>, ViewId), String> {
    let shared = Shared::load(path, Box::new(NoMedia), Settings::scratch(), Rc::new(NullHost), chooser).map_err(|e| e.0)?;
    let v = shared.load_main_view_offscreen();
    Ok((shared, v))
}

/// Draws every window a skin opens when applied (the Skin Chooser picture is often just an image).
fn applied_windows(path: &Path) -> Result<Vec<(String, Canvas)>, String> {
    let shared = Shared::load(path, Box::new(NoMedia), Settings::scratch(), Rc::new(NullHost), false).map_err(|e| e.0)?;
    shared.open_main_view();
    shared.tick(Instant::now());
    Ok(shared.read(|s| {
        s.open_views()
            .filter_map(|v| {
                let id = s.view(v).id.clone().filter(|id| !id.is_empty()).unwrap_or_else(|| format!("view{v}"));
                Some((id, s.render_view(v, 1.0)?))
            })
            .collect()
    }))
}

pub fn run(source: &Path, out: &Path) -> std::io::Result<Summary> {
    std::fs::create_dir_all(out)?;
    crate::set_quiet(true);
    let mut report = String::new();
    let mut summary = Summary::default();
    for file in skin_files(source) {
        let name = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let result = catch_unwind(AssertUnwindSafe(|| -> Outcome {
            // Script errors as when applied (player present); the picture as the Skin Chooser shows it.
            let (applied, v) = load_offscreen(&file, false)?;
            let errors = applied.errors.borrow().clone();
            let tree = applied.read(|s| dump_tree(s, v));
            std::fs::write(out.join(format!("{name}.tree.txt")), tree).map_err(|e| e.to_string())?;
            let (preview, pv) = load_offscreen(&file, true)?;
            let size = preview.read(|s| s.render_view(pv, 1.0)).map(|c| save_png(&trim(&c), &out.join(format!("{name}.png"))));
            for (view, canvas) in applied_windows(&file)? {
                save_png(&trim(&canvas), &out.join(format!("{name}.{view}.png")));
            }
            Ok((errors, size.flatten()))
        }));
        match result {
            Ok(Ok((errors, size))) if errors.is_empty() => {
                summary.ok += 1;
                let (w, h) = size.unwrap_or_default();
                let _ = writeln!(report, "OK      {name}  ({w}x{h})");
            }
            Ok(Ok((errors, _))) => {
                summary.with_errors += 1;
                let _ = writeln!(report, "ERRORS  {name}: {} script error(s)", errors.len());
                for e in errors.iter().take(8) {
                    let _ = writeln!(report, "          {e}");
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
    let _ = writeln!(report, "\n{} OK, {} with script errors, {} failed", summary.ok, summary.with_errors, summary.failed);
    std::fs::write(out.join("report.txt"), &report)?;
    print!("{report}");
    Ok(summary)
}

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
