//! Downloads over HTTPS (the update check, the archive.org skin collection) through the system's
//! `curl`, which macOS ships: it brings TLS, proxies and redirects without a networking stack here.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const USER_AGENT: &str = concat!("WebSkinPlayer-Mac/", env!("CARGO_PKG_VERSION"));

fn curl() -> Command {
    let path = if Path::new("/usr/bin/curl").exists() { "/usr/bin/curl" } else { "curl" };
    let mut c = Command::new(path);
    c.args(["--fail", "--silent", "--show-error", "--location", "--proto", "=https", "--user-agent", USER_AGENT]);
    c.stdin(Stdio::null());
    c
}

/// Fetches a URL into memory (JSON, a screenshot), giving up after `timeout`.
pub fn get(url: &str, headers: &[&str], timeout: Duration) -> Result<Vec<u8>, String> {
    let mut c = curl();
    c.arg("--max-time").arg(timeout.as_secs().max(1).to_string());
    for h in headers {
        c.arg("--header").arg(h);
    }
    let out = c.arg(url).output().map_err(|e| format!("curl: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("download failed ({})", out.status) } else { err })
    }
}

/// How a download is going.
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    Running { bytes: u64 },
    Done(Result<PathBuf, String>),
}

/// A file downloading in the background: into `dest` once complete and checked, never half-written.
pub struct Download {
    pub dest: PathBuf,
    pub expected: u64,
    state: Arc<Mutex<Option<Result<PathBuf, String>>>>,
    bytes: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
}

impl Download {
    /// Starts fetching `url` into `dest`. `check` runs on the finished file (a skin must be a zip, say).
    pub fn start(url: &str, dest: &Path, expected: u64, check: fn(&Path) -> Result<(), String>) -> Download {
        let state = Arc::new(Mutex::new(None));
        let bytes = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let d = Download { dest: dest.to_path_buf(), expected, state: state.clone(), bytes: bytes.clone(), cancel: cancel.clone() };
        let (url, dest) = (url.to_string(), dest.to_path_buf());
        std::thread::spawn(move || {
            let result = fetch(&url, &dest, &bytes, &cancel, check);
            *state.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        });
        d
    }

    pub fn progress(&self) -> Progress {
        match self.state.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            Some(done) => Progress::Done(done),
            None => Progress::Running { bytes: self.bytes.load(Ordering::Relaxed) },
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn fetch(url: &str, dest: &Path, bytes: &AtomicU64, cancel: &AtomicBool, check: fn(&Path) -> Result<(), String>) -> Result<PathBuf, String> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let result = (|| {
        let mut child: Child = curl()
            .arg("--output")
            .arg(&part)
            .args(["--write-out", "%{http_code} %{content_type} %{url_effective}"])
            .arg(url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl: {e}"))?;
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    let mut err = String::new();
                    if let Some(mut s) = child.stderr.take() {
                        let _ = std::io::Read::read_to_string(&mut s, &mut err);
                    }
                    return Err(if err.trim().is_empty() { format!("download failed ({status})") } else { err.trim().to_string() });
                }
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("cancelled".into());
            }
            bytes.store(std::fs::metadata(&part).map_or(0, |m| m.len()), Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(100));
        }
        bytes.store(std::fs::metadata(&part).map_or(0, |m| m.len()), Ordering::Relaxed);
        if let Err(e) = check(&part) {
            // Say what came instead (an error page, a redirect), for the log.
            let mut answer = String::new();
            if let Some(mut s) = child.stdout.take() {
                let _ = std::io::Read::read_to_string(&mut s, &mut answer);
            }
            let start: String = std::fs::read(&part).map(|b| String::from_utf8_lossy(&b[..b.len().min(80)]).into_owned()).unwrap_or_default();
            crate::log(&format!("{url}: {e} (got {answer}: {start:?})"));
            return Err(e);
        }
        std::fs::rename(&part, dest).map_err(|e| e.to_string())?;
        Ok(dest.to_path_buf())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// A zip (a .wmz skin, an app update): anything else is an error page or a broken transfer. Some skins in
/// the archive.org collection have a damaged first header (WMP, and the engine, read them anyway), so a zip
/// is also one whose end-of-directory record is there.
pub fn check_zip(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("the download is empty".into());
    }
    let tail = &bytes[bytes.len().saturating_sub(65_557)..];
    if bytes.starts_with(b"PK") || tail.windows(4).any(|w| w == b"PK\x05\x06") { Ok(()) } else { Err("the download isn't a skin or zip file".into()) }
}

/// The SHA-256 of a file, as lower-case hex (through the system's shasum).
pub fn sha256(path: &Path) -> Result<String, String> {
    let out = Command::new("/usr/bin/shasum").args(["-a", "256"]).arg(path).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace().next().filter(|h| h.len() == 64).map(str::to_lowercase).ok_or_else(|| "can't compute the checksum".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zip_passes_and_an_error_page_does_not() {
        let dir = std::env::temp_dir().join(format!("webskin-net-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.wmz"), b"PK\x03\x04rest").unwrap();
        std::fs::write(dir.join("b.wmz"), b"<html>404</html>").unwrap();
        // A damaged first header, as Need for Speed Underground has, with the directory's end intact.
        std::fs::write(dir.join("c.wmz"), b"\x01\x00\x01\x00\x14\x00 data PK\x05\x06\x00\x00").unwrap();
        std::fs::write(dir.join("d.wmz"), b"").unwrap();
        assert!(check_zip(&dir.join("a.wmz")).is_ok());
        assert!(check_zip(&dir.join("b.wmz")).is_err());
        assert!(check_zip(&dir.join("c.wmz")).is_ok());
        assert!(check_zip(&dir.join("d.wmz")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
