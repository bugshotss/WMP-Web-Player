//! The Mac's "Now Playing" session (Spotify, Music, a browser tab, ...) as the skins' media source.
//!
//! Reading it needs an entitled Apple binary since macOS 15.4, so a `/usr/bin/perl` child loads
//! the `nowplaying-host` library and streams JSON lines. Commands and seeking aren't restricted and
//! go straight to MediaRemote from here.

use super::volume;
use crate::images::Artwork;
use crate::player::{Command, MediaSnapshot, MediaSource, MediaStatus};
use base64::Engine;
use objc2_app_kit::NSRunningApplication;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CString, c_void};
use std::io::BufRead;
use std::path::PathBuf;
use std::process::{Child, Command as Process, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const PERL_LOADER: &str = r#"use DynaLoader; my $lib = DynaLoader::dl_load_file($ARGV[0], 0) or die DynaLoader::dl_error(); my $sym = DynaLoader::dl_find_symbol($lib, "webskin_nowplaying_stream") or die "no stream symbol"; &{DynaLoader::dl_install_xsub("main::stream", $sym)}();"#;

#[derive(Default)]
struct Live {
    info: serde_json::Map<String, serde_json::Value>,
    artwork: Option<Arc<Artwork>>,
    artwork_version: u64,
}

type SendCommandFn = unsafe extern "C" fn(u32, *mut c_void) -> bool;
type SetElapsedFn = unsafe extern "C" fn(f64);
type SetModeFn = unsafe extern "C" fn(i32);

/// MediaRemote's command numbers.
mod command {
    pub const PLAY: u32 = 0;
    pub const PAUSE: u32 = 1;
    pub const TOGGLE_PLAY_PAUSE: u32 = 2;
    pub const STOP: u32 = 3;
    pub const NEXT: u32 = 4;
    pub const PREVIOUS: u32 = 5;
    pub const ADVANCE_SHUFFLE: u32 = 6;
    pub const ADVANCE_REPEAT: u32 = 7;
}

/// MediaRemote's shuffle modes (1 off, 2 albums, 3 tracks) and repeat modes (1 off, 2 the track, 3 the
/// playlist). WMP's shuffle is tracks and its loop is the playlist.
const MODE_OFF: i32 = 1;
const MODE_ON: i32 = 3;

#[derive(Clone, Copy, Default)]
struct MediaRemote {
    send_command: Option<SendCommandFn>,
    set_elapsed: Option<SetElapsedFn>,
    set_shuffle: Option<SetModeFn>,
    set_repeat: Option<SetModeFn>,
}

pub struct NowPlaying {
    live: Arc<Mutex<Live>>,
    helper: RefCell<Option<Child>>,
    remote: MediaRemote,
    app_names: RefCell<HashMap<i64, String>>,
    artwork: RefCell<Option<(u64, Rc<Artwork>)>>,
    /// Shuffle and repeat as last set from here, for apps that take the command but don't report the mode.
    shuffle: Cell<bool>,
    repeat: Cell<bool>,
}

/// The helper library: next to the executable (cargo builds) or in the app's Frameworks folder.
fn helper_library() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("libnowplaying_host.dylib"), dir.join("../Frameworks/libnowplaying_host.dylib")].into_iter().find(|p| p.exists())
}

impl NowPlaying {
    pub fn start() -> Rc<Self> {
        let live = Arc::new(Mutex::new(Live::default()));
        let helper = helper_library().and_then(|lib| {
            let child = Process::new("/usr/bin/perl")
                .arg("-e")
                .arg(PERL_LOADER)
                .arg(&lib)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn();
            match child {
                Ok(mut c) => {
                    let out = c.stdout.take()?;
                    let live = live.clone();
                    std::thread::spawn(move || read_lines(std::io::BufReader::new(out), &live));
                    Some(c)
                }
                Err(e) => {
                    crate::log(&format!("Now Playing helper failed to start: {e}"));
                    None
                }
            }
        });
        if helper.is_none() {
            crate::log("Now Playing helper (libnowplaying_host.dylib) not found; skins won't see what's playing");
        }
        Rc::new(NowPlaying {
            live,
            helper: RefCell::new(helper),
            remote: media_remote(),
            app_names: RefCell::default(),
            artwork: RefCell::default(),
            shuffle: Cell::new(false),
            repeat: Cell::new(false),
        })
    }

    fn app_name(&self, pid: i64) -> String {
        if pid <= 0 {
            return String::new();
        }
        self.app_names
            .borrow_mut()
            .entry(pid)
            .or_insert_with(|| {
                NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)
                    .and_then(|a| a.localizedName())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "Now Playing".into())
            })
            .clone()
    }

    fn send(&self, code: u32) {
        if let Some(f) = self.remote.send_command {
            unsafe { f(code, std::ptr::null_mut()) };
        }
    }

    /// A shuffle or repeat mode the playing app reports (None when it doesn't).
    fn mode(&self, key: &str) -> Option<i64> {
        let l = self.live.lock().unwrap_or_else(|e| e.into_inner());
        l.info.get(key).and_then(serde_json::Value::as_f64).map(|v| v as i64).filter(|v| *v > 0)
    }

    /// Sets shuffle or repeat: straight to the mode when MediaRemote can, else by stepping the app's mode
    /// (which only works out when the mode it reports, or the one last set from here, is known).
    fn set_mode(&self, on: bool, setter: Option<SetModeFn>, advance: u32, key: &str, remembered: &Cell<bool>) {
        let was = self.mode(key).map_or(remembered.get(), |m| m > MODE_OFF.into());
        remembered.set(on);
        match setter {
            Some(f) => unsafe { f(if on { MODE_ON } else { MODE_OFF }) },
            None if was != on => self.send(advance),
            None => {}
        }
    }

    /// Whether shuffle (or repeat) is on: as the app reports it, or as last set from here.
    fn mode_on(&self, key: &str, remembered: &Cell<bool>) -> bool {
        self.mode(key).map_or(remembered.get(), |m| m > MODE_OFF.into())
    }
}

impl Drop for NowPlaying {
    fn drop(&mut self) {
        if let Some(mut c) = self.helper.borrow_mut().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn media_remote() -> MediaRemote {
    unsafe {
        let Ok(path) = CString::new("/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote") else { return MediaRemote::default() };
        let h = libc::dlopen(path.as_ptr(), libc::RTLD_NOW);
        if h.is_null() {
            return MediaRemote::default();
        }
        let sym = |name: &str| CString::new(name).ok().map(|c| libc::dlsym(h, c.as_ptr())).filter(|p| !p.is_null());
        MediaRemote {
            send_command: sym("MRMediaRemoteSendCommand").map(|p| std::mem::transmute::<*mut c_void, SendCommandFn>(p)),
            set_elapsed: sym("MRMediaRemoteSetElapsedTime").map(|p| std::mem::transmute::<*mut c_void, SetElapsedFn>(p)),
            set_shuffle: sym("MRMediaRemoteSetShuffleMode").map(|p| std::mem::transmute::<*mut c_void, SetModeFn>(p)),
            set_repeat: sym("MRMediaRemoteSetRepeatMode").map(|p| std::mem::transmute::<*mut c_void, SetModeFn>(p)),
        }
    }
}

fn read_lines(reader: impl BufRead, live: &Mutex<Live>) {
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(serde_json::Value::Object(mut info)) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let artwork = info.remove("artwork");
        let mut l = live.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(art) = artwork {
            l.artwork = art.as_str().and_then(decode_artwork).map(Arc::new);
            l.artwork_version += 1;
        }
        l.info = info;
    }
}

fn decode_artwork(b64: &str) -> Option<Artwork> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let argb = img.pixels().map(|p| 0xFF00_0000 | (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])).collect();
    Some(Artwork { width: img.width(), height: img.height(), argb })
}

fn now_unix() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// One skin's handle on the shared session.
pub struct Source(pub Rc<NowPlaying>);

impl MediaSource for Source {
    fn snapshot(&self) -> MediaSnapshot {
        let (info, version) = {
            let l = self.0.live.lock().unwrap_or_else(|e| e.into_inner());
            (l.info.clone(), l.artwork_version)
        };
        let text = |k: &str| info.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let pid = info.get("pid").and_then(serde_json::Value::as_i64).unwrap_or(0);
        let playing = info.get("playing").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let session = pid > 0;
        MediaSnapshot {
            title: text("title"),
            artist: text("artist"),
            album: text("album"),
            source_app: self.0.app_name(pid),
            status: match (session, playing) {
                (true, true) => MediaStatus::Playing,
                (true, false) => MediaStatus::Paused,
                _ => MediaStatus::None,
            },
            duration: info.get("duration").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
            can_next: session,
            can_previous: session,
            can_seek: session && self.0.remote.set_elapsed.is_some(),
            // Apps that don't do shuffle or repeat ignore the command, as browsers' players did on Windows.
            can_shuffle: session && self.0.remote.send_command.is_some(),
            can_repeat: session && self.0.remote.send_command.is_some(),
            shuffle: session && self.0.mode_on("shuffleMode", &self.0.shuffle),
            repeat: session && self.0.mode_on("repeatMode", &self.0.repeat),
            artwork_version: version,
        }
    }

    fn position(&self) -> f64 {
        let l = self.0.live.lock().unwrap_or_else(|e| e.into_inner());
        let num = |k: &str| l.info.get(k).and_then(serde_json::Value::as_f64);
        let elapsed = num("elapsed").unwrap_or(0.0);
        let playing = l.info.get("playing").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let since = num("timestamp").map_or(0.0, |t| (now_unix() - t).max(0.0));
        let rate = if playing { num("rate").filter(|r| *r > 0.0).unwrap_or(1.0) } else { 0.0 };
        let pos = elapsed + since * rate;
        num("duration").filter(|d| *d > 0.0).map_or(pos, |d| pos.min(d))
    }

    fn artwork(&self) -> Option<Rc<Artwork>> {
        let (art, version) = {
            let l = self.0.live.lock().unwrap_or_else(|e| e.into_inner());
            (l.artwork.clone(), l.artwork_version)
        };
        let mut cache = self.0.artwork.borrow_mut();
        if let Some((_, a)) = cache.as_ref().filter(|(v, _)| *v == version) {
            return Some(a.clone());
        }
        let rc = art.map(|a| Rc::new(Artwork { width: a.width, height: a.height, argb: a.argb.clone() }));
        *cache = rc.clone().map(|a| (version, a));
        rc
    }

    fn command(&self, c: Command) {
        match c {
            Command::Play => self.0.send(command::PLAY),
            Command::Pause => self.0.send(command::PAUSE),
            Command::TogglePlayPause => self.0.send(command::TOGGLE_PLAY_PAUSE),
            Command::Stop => self.0.send(command::STOP),
            Command::Next => self.0.send(command::NEXT),
            Command::Previous => self.0.send(command::PREVIOUS),
            Command::FastForward => self.seek(self.position() + 10.0),
            Command::Rewind => self.seek((self.position() - 10.0).max(0.0)),
        }
    }

    fn seek(&self, seconds: f64) {
        if let Some(f) = self.0.remote.set_elapsed {
            unsafe { f(seconds) };
        }
    }

    fn set_shuffle(&self, on: bool) {
        let np = &self.0;
        np.set_mode(on, np.remote.set_shuffle, command::ADVANCE_SHUFFLE, "shuffleMode", &np.shuffle);
    }

    fn set_repeat(&self, on: bool) {
        let np = &self.0;
        np.set_mode(on, np.remote.set_repeat, command::ADVANCE_REPEAT, "repeatMode", &np.repeat);
    }

    fn volume(&self) -> f64 {
        volume::get().unwrap_or(0.5)
    }

    fn set_volume(&self, v: f64) {
        volume::set(v);
    }

    fn mute(&self) -> bool {
        volume::muted().unwrap_or(false)
    }

    fn set_mute(&self, on: bool) {
        volume::set_muted(on);
    }
}
