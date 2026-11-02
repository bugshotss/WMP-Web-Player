//! Skin Chooser previews: a skin's main view drawn offscreen as WMP's Skin Chooser showed it (without a
//! player object), plus the title, author and copyright the skin carries. Skins load and run their
//! scripts on one worker thread, so the chooser stays responsive while a big skin opens.

use crate::package::SkinPackage;
use crate::player::NoMedia;
use crate::settings::Settings;
use crate::skin::{NullHost, Shared};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use tiny_skia::Pixmap;

/// What a skin says about itself, from its THEME element.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkinInfo {
    pub title: String,
    pub author: String,
    pub copyright: String,
}

/// Reads a skin's title, author and copyright (cheap: only the .wms is parsed).
pub fn info(path: &Path) -> Option<SkinInfo> {
    let package = SkinPackage::open(path).ok()?;
    let text = package.read_text(&package.definition)?;
    let root = crate::wms::parse(&text);
    let theme = root.children.iter().find(|n| n.tag == "THEME").unwrap_or(&root);
    let attr = |name: &str| theme.attr(name).unwrap_or_default().trim().to_string();
    let mut title = attr("title");
    // Some themes carry a resource id or a script instead of a real title.
    if title.to_lowercase().starts_with("res:") || title.to_lowercase().starts_with("jscript:") {
        title.clear();
    }
    Some(SkinInfo { title, author: attr("author"), copyright: attr("copyright") })
}

/// Draws a skin's preview at `scale` device pixels per skin pixel, trimmed to what it shows.
pub fn render(path: &Path, scale: f32) -> Result<Pixmap, String> {
    catch_unwind(AssertUnwindSafe(|| {
        let shared = Shared::load(path, Box::new(NoMedia), Settings::scratch(), Rc::new(NullHost), true).map_err(|e| e.0)?;
        let v = shared.load_main_view_offscreen();
        let canvas = shared.read(|s| s.render_view(v, scale)).ok_or("The skin draws nothing")?;
        shared.close();
        Ok(crate::runner::trim(&canvas))
    }))
    .unwrap_or_else(|_| Err("The skin couldn't be drawn".to_string()))
}

/// A finished preview.
pub struct Rendered {
    pub path: PathBuf,
    pub image: Result<Pixmap, String>,
}

/// The preview worker: ask for skins with [`Previews::request`], collect them with [`Previews::poll`].
pub struct Previews {
    requests: Sender<(PathBuf, f32)>,
    done: Receiver<Rendered>,
}

impl Default for Previews {
    fn default() -> Self {
        Self::new()
    }
}

impl Previews {
    pub fn new() -> Self {
        let (requests, todo) = channel::<(PathBuf, f32)>();
        let (finished, done) = channel();
        std::thread::Builder::new()
            .name("skin previews".into())
            .spawn(move || {
                while let Ok(mut job) = todo.recv() {
                    // Only the newest request matters when several queued up while the user scrolled.
                    while let Ok(newer) = todo.try_recv() {
                        job = newer;
                    }
                    let image = render(&job.0, job.1);
                    if finished.send(Rendered { path: job.0, image }).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Previews { requests, done }
    }

    pub fn request(&self, path: &Path, scale: f32) {
        let _ = self.requests.send((path.to_path_buf(), scale));
    }

    pub fn poll(&self) -> Option<Rendered> {
        self.done.try_recv().ok()
    }
}
