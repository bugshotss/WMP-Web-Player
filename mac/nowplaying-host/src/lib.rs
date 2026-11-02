//! Reads macOS "Now Playing" from inside `/usr/bin/perl`.
//!
//! Since macOS 15.4 MediaRemote only answers now-playing queries from entitled Apple binaries, and
//! perl is one of them. The player starts `perl`, which loads this library with DynaLoader and
//! calls [`webskin_nowplaying_stream`]; it writes one JSON line to stdout whenever anything changes.
//! Commands (play, pause, seek, ...) are not restricted, so the player sends those itself.

use base64::Engine;
use block2::RcBlock;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSData, NSDate, NSDictionary, NSNumber, NSString};
use std::ffi::{CString, c_void};
use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

type InfoBlock = block2::Block<dyn Fn(*const NSDictionary<NSString, AnyObject>)>;
type IntBlock = block2::Block<dyn Fn(i32)>;
type GetInfoFn = unsafe extern "C" fn(*mut c_void, &InfoBlock);
type GetIntFn = unsafe extern "C" fn(*mut c_void, &IntBlock);

unsafe extern "C" {
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
}

struct MediaRemote {
    get_info: GetInfoFn,
    get_pid: GetIntFn,
    get_playing: GetIntFn,
}

impl MediaRemote {
    fn load() -> Option<Self> {
        unsafe {
            let path = CString::new("/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote").ok()?;
            let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW);
            if handle.is_null() {
                return None;
            }
            let sym = |name: &str| {
                let c = CString::new(name).ok()?;
                let p = libc::dlsym(handle, c.as_ptr());
                (!p.is_null()).then_some(p)
            };
            Some(Self {
                get_info: std::mem::transmute::<*mut c_void, GetInfoFn>(sym("MRMediaRemoteGetNowPlayingInfo")?),
                get_pid: std::mem::transmute::<*mut c_void, GetIntFn>(sym("MRMediaRemoteGetNowPlayingApplicationPID")?),
                get_playing: std::mem::transmute::<*mut c_void, GetIntFn>(sym("MRMediaRemoteGetNowPlayingApplicationIsPlaying")?),
            })
        }
    }

    fn int(&self, f: GetIntFn) -> i32 {
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |v: i32| {
            let _ = tx.send(v);
        });
        unsafe { f(dispatch_get_global_queue(0, 0), &block) };
        rx.recv_timeout(Duration::from_secs(2)).unwrap_or(0)
    }

    fn snapshot(&self) -> serde_json::Map<String, serde_json::Value> {
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |d: *const NSDictionary<NSString, AnyObject>| {
            let _ = tx.send(unsafe { d.as_ref() }.map(read_info).unwrap_or_default());
        });
        unsafe { (self.get_info)(dispatch_get_global_queue(0, 0), &block) };
        let mut info = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
        info.insert("pid".into(), self.int(self.get_pid).into());
        info.insert("playing".into(), (self.int(self.get_playing) != 0).into());
        info
    }
}

fn read_info(d: &NSDictionary<NSString, AnyObject>) -> serde_json::Map<String, serde_json::Value> {
    let get = |key: &str| d.objectForKey(&NSString::from_str(&format!("kMRMediaRemoteNowPlayingInfo{key}")));
    let text = |key: &str| get(key).and_then(|o| o.downcast::<NSString>().ok()).map(|s| s.to_string());
    let number = |key: &str| get(key).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.doubleValue());
    let mut out = serde_json::Map::new();
    for (key, name) in [("Title", "title"), ("Artist", "artist"), ("Album", "album"), ("ArtworkIdentifier", "artworkId")] {
        if let Some(v) = text(key) {
            out.insert(name.into(), v.into());
        }
    }
    for (key, name) in [("Duration", "duration"), ("ElapsedTime", "elapsed"), ("PlaybackRate", "rate")] {
        if let Some(v) = number(key) {
            out.insert(name.into(), v.into());
        }
    }
    if let Some(date) = get("Timestamp").and_then(|o| o.downcast::<NSDate>().ok()) {
        out.insert("timestamp".into(), date.timeIntervalSince1970().into());
    }
    if let Some(data) = get("ArtworkData").and_then(|o| o.downcast::<NSData>().ok()) {
        out.insert("artwork".into(), base64::engine::general_purpose::STANDARD.encode(data.to_vec()).into());
    }
    out
}

/// Sends artwork only when it changed; every other field goes out whenever anything differs.
fn stream(mr: &MediaRemote, out: &mut impl Write) -> std::io::Result<()> {
    let mut last = serde_json::Map::new();
    let mut last_artwork: Option<serde_json::Value> = None;
    loop {
        let mut now = mr.snapshot();
        let artwork = now.remove("artwork");
        if now != last || artwork != last_artwork {
            let mut line = now.clone();
            if artwork != last_artwork {
                line.insert("artwork".into(), artwork.clone().unwrap_or(serde_json::Value::Null));
            }
            writeln!(out, "{}", serde_json::Value::Object(line))?;
            out.flush()?;
            last = now;
            last_artwork = artwork;
        }
        std::thread::sleep(Duration::from_millis(250));
        if unsafe { libc::getppid() } == 1 {
            return Ok(());
        }
    }
}

/// Called by perl as an XSUB (`DynaLoader::dl_install_xsub`); the arguments are perl's and unused.
/// Runs until the player closes the pipe.
#[unsafe(no_mangle)]
pub extern "C" fn webskin_nowplaying_stream(_interpreter: *mut c_void, _cv: *mut c_void) {
    let Some(mr) = MediaRemote::load() else {
        eprintln!("MediaRemote is not available");
        return;
    };
    let _ = stream(&mr, &mut std::io::stdout().lock());
}
