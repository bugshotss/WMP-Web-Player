//! WebSkin Player for macOS: Windows Media Player skins for whatever the Mac is playing.

mod images;
mod package;
mod player;
mod render;
mod resources;
pub mod runner;
mod script;
mod settings;
mod skin;
mod text;
mod value;
mod vis;
mod wms;

#[cfg(target_os = "macos")]
pub mod mac;

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static QUIET: AtomicBool = AtomicBool::new(false);

/// Keeps log lines off stderr (the corpus runner collects them itself).
pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

/// Appends a line to the log file in the support folder (and stderr).
pub fn log(line: &str) {
    if !QUIET.load(Ordering::Relaxed) {
        eprintln!("{line}");
    }
    if cfg!(test) {
        return;
    }
    let dir = settings::support_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("log.txt")) {
        let _ = writeln!(f, "{line}");
    }
}
