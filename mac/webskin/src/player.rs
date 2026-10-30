//! The single shared "player" all skins drive: turns the system's now-playing session into the state
//! WMP skins expect (playState, openState, currentPosition, volume 0-100, ...).

use crate::images::Artwork;
use crate::value::Value;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// WMP numeric constants (wmpcore.h) that skins compare against.
pub mod state {
    pub const PS_UNDEFINED: i32 = 0;
    pub const PS_STOPPED: i32 = 1;
    pub const PS_PAUSED: i32 = 2;
    pub const PS_PLAYING: i32 = 3;
    pub const PS_TRANSITIONING: i32 = 9;
    pub const PS_READY: i32 = 10;
    pub const OS_UNDEFINED: i32 = 0;
    pub const OS_PLAYLIST_OPEN_NO_MEDIA: i32 = 6;
    pub const OS_MEDIA_CHANGING: i32 = 8;
    pub const OS_MEDIA_OPEN: i32 = 13;

    /// Every constant skins may name, as globals.
    pub const ALL: &[(&str, i32)] = &[
        ("psUndefined", 0),
        ("psStopped", 1),
        ("psPaused", 2),
        ("psPlaying", 3),
        ("psScanForward", 4),
        ("psScanReverse", 5),
        ("psBuffering", 6),
        ("psWaiting", 7),
        ("psMediaEnded", 8),
        ("psTransitioning", 9),
        ("psReady", 10),
        ("psReconnecting", 11),
        ("osUndefined", 0),
        ("osPlaylistChanging", 1),
        ("osPlaylistLocating", 2),
        ("osPlaylistConnecting", 3),
        ("osPlaylistLoading", 4),
        ("osPlaylistOpening", 5),
        ("osPlaylistOpenNoMedia", 6),
        ("osPlaylistChanged", 7),
        ("osMediaChanging", 8),
        ("osMediaLocating", 9),
        ("osMediaConnecting", 10),
        ("osMediaLoading", 11),
        ("osMediaOpening", 12),
        ("osMediaOpen", 13),
        ("osBeginCodecAcquisition", 14),
        ("osEndCodecAcquisition", 15),
        ("osBeginLicenseAcquisition", 16),
        ("osEndLicenseAcquisition", 17),
        ("osBeginIndividualization", 18),
        ("osEndIndividualization", 19),
        ("osMediaWaiting", 20),
        ("osOpeningUnknownURL", 21),
    ];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaStatus {
    #[default]
    None,
    Paused,
    Playing,
}

/// What the followed app is doing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaSnapshot {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The app's name ("Spotify", "Google Chrome"); empty when nothing has a session.
    pub source_app: String,
    pub status: MediaStatus,
    pub duration: f64,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    pub can_shuffle: bool,
    pub can_repeat: bool,
    pub shuffle: bool,
    pub repeat: bool,
    /// Changes whenever the artwork does.
    pub artwork_version: u64,
}

impl MediaSnapshot {
    pub fn has_session(&self) -> bool {
        !self.source_app.is_empty()
    }

    fn same_track(&self, other: &MediaSnapshot) -> bool {
        self.title == other.title && self.artist == other.artist && self.album == other.album && self.source_app == other.source_app
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Play,
    Pause,
    TogglePlayPause,
    Stop,
    Next,
    Previous,
    FastForward,
    Rewind,
}

/// The system's media session and output volume.
pub trait MediaSource {
    fn snapshot(&self) -> MediaSnapshot;
    fn position(&self) -> f64;
    fn artwork(&self) -> Option<Rc<Artwork>>;
    fn command(&self, c: Command);
    fn seek(&self, seconds: f64);
    fn set_shuffle(&self, on: bool);
    fn set_repeat(&self, on: bool);
    /// Output volume, 0..1.
    fn volume(&self) -> f64;
    fn set_volume(&self, v: f64);
    fn mute(&self) -> bool;
    fn set_mute(&self, on: bool);
}

/// A source with nothing playing (skin previews and tests).
pub struct NoMedia;

impl MediaSource for NoMedia {
    fn snapshot(&self) -> MediaSnapshot {
        MediaSnapshot::default()
    }
    fn position(&self) -> f64 {
        0.0
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

/// What changed, for skins: a property of a WMP object, or a player event.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerChange {
    Prop(&'static str, &'static str),
    Event(&'static str, Vec<Value>),
}

// A skip made outside the player (media keys, Control Center) pauses while the next song loads and
// only then shows its title, so a plain pause is held long enough for that; a song change can take
// several seconds while a web player loads the next song.
const PAUSE_HOLD: Duration = Duration::from_millis(1500);
const SONG_CHANGE_HOLD: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(250);

pub struct PlayerModel {
    pub source: Box<dyn MediaSource>,
    pub play_state: i32,
    pub open_state: i32,
    pub status: String,
    pub media: MediaSnapshot,
    pub position: f64,
    volume: i32,
    mute: bool,
    balance: i32,
    last_poll: Option<Instant>,
    last_volume_poll: Option<Instant>,
    /// When the player's own pause/stop was last pressed: that pause is shown at once.
    commanded_at: Option<Instant>,
    /// When the player's own next/previous was last pressed: the stop that follows is a song change.
    skipped_at: Option<Instant>,
    gap_since: Option<Instant>,
    gap_is_song_change: bool,
    hold_until: Option<Instant>,
    playing_title: String,
    changes: Vec<PlayerChange>,
}

impl PlayerModel {
    pub fn new(source: Box<dyn MediaSource>) -> Self {
        let mut pm = PlayerModel {
            source,
            play_state: state::PS_UNDEFINED,
            open_state: state::OS_UNDEFINED,
            status: String::new(),
            media: MediaSnapshot::default(),
            position: 0.0,
            volume: 50,
            mute: false,
            balance: 0,
            last_poll: None,
            last_volume_poll: None,
            commanded_at: None,
            skipped_at: None,
            gap_since: None,
            gap_is_song_change: false,
            hold_until: None,
            playing_title: String::new(),
            changes: vec![],
        };
        let now = Instant::now();
        pm.media = pm.source.snapshot();
        pm.on_track_changed();
        pm.on_media_changed(now);
        pm.changes.clear();
        pm
    }

    pub fn duration(&self) -> f64 {
        self.media.duration
    }

    fn raise(&mut self, obj: &'static str, prop: &'static str) {
        self.changes.push(PlayerChange::Prop(obj, prop));
    }

    fn event(&mut self, name: &'static str, args: Vec<Value>) {
        self.changes.push(PlayerChange::Event(name, args));
    }

    /// Follows the source; returns what changed since the last call.
    pub fn tick(&mut self, now: Instant) -> Vec<PlayerChange> {
        if self.last_poll.is_none_or(|t| now.duration_since(t) >= POLL) {
            self.last_poll = Some(now);
            let snap = self.source.snapshot();
            if snap != self.media {
                let track = !snap.same_track(&self.media);
                self.media = snap;
                if track {
                    self.on_track_changed();
                }
                self.on_media_changed(now);
            } else if self.hold_until.is_some_and(|t| now >= t) {
                self.on_media_changed(now);
            } else {
                self.poll(now);
            }
        }
        std::mem::take(&mut self.changes)
    }

    fn on_track_changed(&mut self) {
        let m = &self.media;
        let new_open = match (m.has_session(), m.title.is_empty()) {
            (true, false) => state::OS_MEDIA_OPEN,
            (true, true) => state::OS_PLAYLIST_OPEN_NO_MEDIA,
            _ => state::OS_UNDEFINED,
        };
        // Mimic WMP's open sequence so skins that refresh metadata on osMediaOpen do so.
        if self.media.has_session() {
            self.set_open_state(state::OS_MEDIA_CHANGING);
        }
        self.set_open_state(new_open);
        self.raise("currentMedia", "name");
        self.raise("currentMedia", "duration");
        self.raise("currentMedia", "durationString");
        self.raise("player", "currentMedia");
        self.event("MediaChange", vec![Value::Null]);
        self.event("CurrentItemChange", vec![Value::Null]);
    }

    fn set_open_state(&mut self, s: i32) {
        if self.open_state == s {
            return;
        }
        self.open_state = s;
        self.raise("player", "openState");
        self.event("OpenStateChange", vec![s.into()]);
    }

    fn on_media_changed(&mut self, now: Instant) {
        let m = self.media.clone();
        let mut ps = match m.status {
            MediaStatus::Playing => state::PS_PLAYING,
            MediaStatus::Paused => state::PS_PAUSED,
            MediaStatus::None if m.has_session() => state::PS_READY,
            MediaStatus::None => state::PS_UNDEFINED,
        };
        // Web players stop for a moment between songs where WMP went straight on, and skins that react
        // to every play state change would treat each of those as a pause and resume. A stop in
        // mid-play that the player didn't ask for is held and forgotten if playback carries on:
        // briefly for a plain pause, longer when it looks like a song change.
        let mut held = false;
        let commanded_recently = self.commanded_at.is_some_and(|t| now.duration_since(t) <= Duration::from_secs(2));
        if self.play_state == state::PS_PLAYING && ps != state::PS_PLAYING && !commanded_recently {
            let since = *self.gap_since.get_or_insert_with(|| {
                self.gap_is_song_change = self.media.duration > 0.0 && self.media.duration - self.position < 4.0;
                now
            });
            self.gap_is_song_change |= !m.has_session()
                || m.status != MediaStatus::Paused
                || m.title != self.playing_title
                || self.skipped_at.is_some_and(|t| now.duration_since(t) < SONG_CHANGE_HOLD);
            let until = since + if self.gap_is_song_change { SONG_CHANGE_HOLD } else { PAUSE_HOLD };
            if now < until {
                held = true;
                ps = state::PS_PLAYING;
                self.hold_until = Some(until + Duration::from_millis(20));
            }
        }
        if !held {
            if let Some(since) = self.gap_since.filter(|s| now.duration_since(*s) > Duration::from_secs(1)) {
                crate::log(&format!(
                    "Playback stopped for {:.1} s ({}): {}",
                    now.duration_since(since).as_secs_f64(),
                    if self.gap_is_song_change { "song change" } else { "pause" },
                    if ps == state::PS_PLAYING { "carried on".to_string() } else { format!("shown as play state {ps}") }
                ));
            }
            self.gap_since = None;
            self.hold_until = None;
        }
        if m.status == MediaStatus::Playing {
            self.playing_title = m.title.clone();
        }
        if ps != self.play_state {
            self.play_state = ps;
            self.raise("player", "playState");
            self.event("PlayStateChange", vec![ps.into()]);
        }
        let status = match ps {
            state::PS_PLAYING => "Playing",
            state::PS_PAUSED => "Paused",
            state::PS_STOPPED => "Stopped",
            state::PS_TRANSITIONING => "Transitioning",
            state::PS_READY => "Ready",
            _ if m.has_session() => "Ready",
            _ => "Play something in Spotify, Music or your browser",
        };
        if status != self.status {
            self.status = status.to_string();
            self.raise("player", "status");
            self.event("StatusChange", vec![]);
        }
        self.raise("controls", "isAvailable");
        self.raise("currentMedia", "duration");
        self.raise("currentMedia", "durationString");
        self.poll(now);
        self.poll_volume(now, true);
    }

    fn poll(&mut self, now: Instant) {
        let pos = self.source.position();
        if (pos - self.position).abs() > 0.05 {
            let old = self.position;
            self.position = pos;
            self.raise("controls", "currentPosition");
            self.raise("controls", "currentPositionString");
            self.raise("controls", "currentPositionTimecode");
            if (pos - old).abs() > 1.5 {
                self.event("PositionChange", vec![old.into(), pos.into()]);
            }
        }
        if self.last_volume_poll.is_none_or(|t| now.duration_since(t) > Duration::from_secs(1)) {
            self.poll_volume(now, false);
        }
    }

    fn poll_volume(&mut self, now: Instant, force: bool) {
        self.last_volume_poll = Some(now);
        let v = (self.source.volume() * 100.0).round() as i32;
        let mute = self.source.mute();
        if v != self.volume || force {
            self.volume = v;
            self.raise("settings", "volume");
        }
        if mute != self.mute || force {
            self.mute = mute;
            self.raise("settings", "mute");
        }
    }

    // ---- WMP object model surface --------------------------------------------------------

    pub fn volume(&self) -> i32 {
        self.volume
    }

    pub fn set_volume(&mut self, v: i32) {
        let v = v.clamp(0, 100);
        if v == self.volume {
            return;
        }
        self.volume = v;
        self.source.set_volume(f64::from(v) / 100.0);
        self.raise("settings", "volume");
    }

    pub fn mute(&self) -> bool {
        self.mute
    }

    pub fn set_mute(&mut self, on: bool) {
        if on == self.mute {
            return;
        }
        self.mute = on;
        self.source.set_mute(on);
        self.raise("settings", "mute");
    }

    /// Stored only: an app's stream balance can't be changed from outside.
    pub fn balance(&self) -> i32 {
        self.balance
    }

    pub fn set_balance(&mut self, v: i32) {
        let v = v.clamp(-100, 100);
        if v != self.balance {
            self.balance = v;
            self.raise("settings", "balance");
        }
    }

    pub fn set_position(&mut self, seconds: f64) {
        if !self.media.can_seek {
            return;
        }
        self.position = seconds.clamp(0.0, self.duration().max(0.0));
        self.source.seek(self.position);
        self.raise("controls", "currentPosition");
        self.raise("controls", "currentPositionString");
    }

    pub fn play(&mut self) {
        if self.media.status != MediaStatus::Playing {
            self.source.command(Command::Play);
        }
    }

    pub fn pause(&mut self) {
        self.commanded_at = Some(Instant::now());
        self.source.command(Command::Pause);
    }

    pub fn stop(&mut self) {
        self.commanded_at = Some(Instant::now());
        self.source.command(Command::Stop);
    }

    pub fn next(&mut self) {
        self.skipped_at = Some(Instant::now());
        self.source.command(Command::Next);
    }

    pub fn previous(&mut self) {
        self.skipped_at = Some(Instant::now());
        self.source.command(Command::Previous);
    }

    pub fn fast_forward(&mut self) {
        self.source.command(Command::FastForward);
    }

    pub fn fast_reverse(&mut self) {
        self.source.command(Command::Rewind);
    }

    pub fn toggle_play_pause(&mut self) {
        self.commanded_at = Some(Instant::now());
        self.source.command(Command::TogglePlayPause);
    }

    pub fn is_available(&self, what: &str) -> bool {
        let m = &self.media;
        match what.to_lowercase().as_str() {
            "play" => m.has_session() && m.status != MediaStatus::Playing,
            "pause" => m.status == MediaStatus::Playing,
            "stop" => m.status != MediaStatus::None,
            "next" => m.can_next,
            "previous" => m.can_previous,
            "fastforward" | "fastreverse" => m.can_seek && m.status == MediaStatus::Playing,
            "currentposition" | "seek" => m.can_seek,
            "currentitem" => m.has_session(),
            _ => false,
        }
    }

    pub fn shuffle(&self) -> bool {
        self.media.shuffle
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if self.media.can_shuffle {
            self.source.set_shuffle(on);
        }
        self.raise("settings", "shuffle");
        self.event("ModeChange", vec!["shuffle".into(), on.into()]);
    }

    pub fn repeat(&self) -> bool {
        self.media.repeat
    }

    pub fn set_repeat(&mut self, on: bool) {
        if self.media.can_repeat {
            self.source.set_repeat(on);
        }
        self.raise("settings", "loop");
        self.event("ModeChange", vec!["loop".into(), on.into()]);
    }

    /// Changes made by the setters above (volume, balance, ...) that skins should hear about.
    pub fn take_changes(&mut self) -> Vec<PlayerChange> {
        std::mem::take(&mut self.changes)
    }

    /// WMP getItemInfo attribute names mapped onto the session metadata.
    pub fn item_info(&self, attribute: &str) -> String {
        let m = &self.media;
        match attribute.to_lowercase().as_str() {
            "title" | "name" => m.title.clone(),
            "author" | "artist" | "wm/albumartist" | "displayartist" => m.artist.clone(),
            "album" | "wm/albumtitle" | "albumid" => m.album.clone(),
            "duration" => format!("{}", self.duration() as i64),
            "sourceurl" => self.source_url(),
            "mediatype" => "audio".into(),
            _ => String::new(),
        }
    }

    pub fn source_url(&self) -> String {
        if self.media.has_session() { format!("web:{}", self.media.source_app) } else { String::new() }
    }
}

pub fn format_time(seconds: f64) -> String {
    if seconds.is_nan() || seconds <= 0.0 {
        return "00:00".into();
    }
    let t = seconds as u64;
    let (h, m, s) = (t / 3600, (t / 60) % 60, t % 60);
    if h >= 1 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake(Rc<RefCell<MediaSnapshot>>);

    impl MediaSource for Fake {
        fn snapshot(&self) -> MediaSnapshot {
            self.0.borrow().clone()
        }
        fn position(&self) -> f64 {
            10.0
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

    fn playing(title: &str) -> MediaSnapshot {
        MediaSnapshot { title: title.into(), source_app: "Spotify".into(), status: MediaStatus::Playing, duration: 200.0, ..Default::default() }
    }

    #[test]
    fn holds_a_brief_pause_between_songs_and_reports_the_new_track() {
        let snap = Rc::new(RefCell::new(playing("One")));
        let mut pm = PlayerModel::new(Box::new(Fake(snap.clone())));
        assert_eq!(pm.play_state, state::PS_PLAYING);
        let t0 = Instant::now();

        snap.borrow_mut().status = MediaStatus::Paused;
        snap.borrow_mut().title = "Two".into();
        let changes = pm.tick(t0 + Duration::from_millis(300));
        assert_eq!(pm.play_state, state::PS_PLAYING, "the gap is held");
        assert!(changes.contains(&PlayerChange::Event("MediaChange", vec![Value::Null])));

        *snap.borrow_mut() = playing("Two");
        let changes = pm.tick(t0 + Duration::from_millis(600));
        assert!(!changes.iter().any(|c| matches!(c, PlayerChange::Event("PlayStateChange", _))));
    }

    #[test]
    fn a_pause_that_lasts_is_shown() {
        let snap = Rc::new(RefCell::new(playing("One")));
        let mut pm = PlayerModel::new(Box::new(Fake(snap.clone())));
        let t0 = Instant::now();
        snap.borrow_mut().status = MediaStatus::Paused;
        pm.tick(t0 + Duration::from_millis(300));
        assert_eq!(pm.play_state, state::PS_PLAYING);
        pm.tick(t0 + Duration::from_millis(2000));
        assert_eq!(pm.play_state, state::PS_PAUSED);
    }

    #[test]
    fn formats_times() {
        assert_eq!(format_time(0.0), "00:00");
        assert_eq!(format_time(75.4), "01:15");
        assert_eq!(format_time(3725.0), "1:02:05");
    }
}
