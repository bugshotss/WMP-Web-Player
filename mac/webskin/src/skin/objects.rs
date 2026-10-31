//! WMP's scripting object model (player, controls, settings, currentMedia, theme, mediacenter,
//! event, ...) over the shared player, plus the generic get/set/call every script access goes through.

use super::{Deferred, Skin, element};
use crate::player::format_time;
use crate::value::{ObjRef, Value};
use std::collections::HashMap;

/// Parameter names of WMP player events, in order.
pub fn event_params(name: &str) -> Option<&'static [&'static str]> {
    const EVENTS: &[(&str, &[&str])] = &[
        ("OpenStateChange", &["NewState"]),
        ("PlayStateChange", &["NewState"]),
        ("StatusChange", &[]),
        ("MediaChange", &["Item"]),
        ("CurrentItemChange", &["pdispMedia"]),
        ("CurrentMediaItemAvailable", &["bstrItemName"]),
        ("CurrentPlaylistChange", &["change"]),
        ("CurrentPlaylistItemAvailable", &["bstrItemName"]),
        ("PlaylistChange", &["Playlist", "change"]),
        ("ModeChange", &["ModeName", "NewValue"]),
        ("PositionChange", &["oldPosition", "newPosition"]),
        ("Disconnect", &["Result"]),
        ("Buffering", &["Start"]),
        ("Error", &[]),
        ("MediaError", &["pMediaObject"]),
        ("Warning", &["WarningType", "Param", "Description"]),
        ("EndOfStream", &["Result"]),
        ("MarkerHit", &["MarkerNum"]),
        ("ScriptCommand", &["scType", "Param"]),
        ("NewStream", &[]),
        ("DurationUnitChange", &["NewDurationUnit"]),
        ("MediaCollectionChange", &[]),
        ("PlayerReconnect", &[]),
        ("DomainChange", &["strDomain"]),
        ("OpenPlaylistSwitch", &["pItem"]),
    ];
    EVENTS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, p)| *p)
}

const MEDIA_ATTRIBUTES: &[&str] = &["Title", "Author", "WM/AlbumTitle", "Duration", "SourceURL", "MediaType"];

/// (properties, methods) of each shared object, lower-case.
fn members(o: ObjRef) -> (&'static [&'static str], &'static [&'static str]) {
    match o {
        ObjRef::Player => (
            &[
                "url",
                "playstate",
                "openstate",
                "status",
                "currentmedia",
                "currentplaylist",
                "controls",
                "settings",
                "network",
                "dvd",
                "error",
                "cdromcollection",
                "mediacollection",
                "playlistcollection",
                "versioninfo",
                "isonline",
                "isremote",
                "enabled",
                "fullscreen",
                "enablecontextmenu",
                "uimode",
                "stretchtofit",
                "windowlessvideo",
                "closedcaption",
                "playerapplication",
            ],
            &["launchurl", "close", "newmedia", "newplaylist", "openplayer"],
        ),
        ObjRef::Controls => (
            &[
                "currentposition",
                "currentpositionstring",
                "currentpositiontimecode",
                "currentitem",
                "currentmarker",
                "audiolanguagecount",
                "currentaudiolanguage",
                "currentaudiolanguageindex",
            ],
            &["play", "pause", "stop", "next", "previous", "fastforward", "fastreverse", "step", "playitem", "isavailable"],
        ),
        ObjRef::Settings => (
            &[
                "volume",
                "mute",
                "balance",
                "shuffle",
                "loop",
                "rate",
                "autostart",
                "playcount",
                "baseurl",
                "defaultframe",
                "enableerrordialogs",
                "invokeurls",
                "mediaaccessrights",
            ],
            &["getmode", "setmode", "isavailable", "requestmediaaccessrights"],
        ),
        ObjRef::CurrentMedia => (
            &["name", "duration", "durationstring", "sourceurl", "imagesourcewidth", "imagesourceheight", "markercount", "attributecount"],
            &[
                "getiteminfo",
                "getiteminfobyatom",
                "getattributename",
                "getattributecountbytype",
                "getiteminfobytype",
                "setiteminfo",
                "isidentical",
                "ismemberof",
                "isreadonlyitem",
                "getmarkertime",
                "getmarkername",
            ],
        ),
        ObjRef::HistoryItem(_) => (&["name", "duration", "durationstring", "sourceurl"], &["getiteminfo", "isidentical"]),
        ObjRef::CurrentPlaylist => (
            &["count", "name", "attributecount"],
            &["item", "getiteminfo", "setiteminfo", "getattributename", "isidentical", "appenditem", "insertitem", "removeitem", "clear", "moveitem"],
        ),
        ObjRef::Network => (
            &[
                "bufferingprogress",
                "downloadprogress",
                "receptionquality",
                "bandwidth",
                "bitrate",
                "maxbitrate",
                "maxbandwidth",
                "bufferingcount",
                "bufferingtime",
                "encodedframerate",
                "framerate",
                "framesskipped",
                "lostpackets",
                "receivedpackets",
                "recoveredpackets",
                "sourceprotocol",
            ],
            &["getproxyname", "getproxysettings"],
        ),
        ObjRef::Dvd => (&["domain"], &["isavailable", "topmenu", "titlemenu", "back", "resume"]),
        ObjRef::Error => (&["errorcount"], &["item", "clearerrorqueue", "webhelp"]),
        ObjRef::Cdroms => (&["count"], &["item", "getbydrivespecifier"]),
        ObjRef::MediaCollection | ObjRef::PlaylistCollection => (
            &[],
            &[
                "add",
                "getall",
                "getbyname",
                "getbygenre",
                "getbyauthor",
                "getbyalbum",
                "getbyattribute",
                "remove",
                "newplaylist",
                "importplaylist",
                "getattributestringcollection",
                "isdeleted",
                "setdeleted",
            ],
        ),
        ObjRef::MediaCenter => (
            &[
                "effecttype",
                "effectpreset",
                "effectpresettitle",
                "effecttitle",
                "videozoom",
                "videostretchtofit",
                "showeffects",
                "showtitles",
                "contrastmode",
            ],
            &[],
        ),
        ObjRef::Theme => (
            &["currentviewid", "title", "author", "copyright", "version"],
            &[
                "openview",
                "openviewrelative",
                "closeview",
                "loadpreference",
                "savepreference",
                "loadstring",
                "opendialog",
                "playsound",
                "stopsound",
                "showerrordialog",
                "logstring",
                "messagebox",
            ],
        ),
        ObjRef::Event => (&["screenwidth", "screenheight", "screenleft", "screentop", "fullscreenwidth", "fullscreenheight"], &[]),
        ObjRef::Element { .. } => (&[], &[]),
    }
}

/// The event object's initial values.
pub fn new_event() -> HashMap<String, Value> {
    let mut e = HashMap::new();
    for n in ["keycode", "x", "y", "offsetx", "offsety", "screenx", "screeny", "button", "wheeldelta"] {
        e.insert(n.to_string(), 0.0.into());
    }
    for n in ["shiftkey", "ctrlkey", "altkey", "cancelbubble"] {
        e.insert(n.to_string(), false.into());
    }
    e.insert("returnvalue".into(), true.into());
    e
}

pub fn set_modifiers(e: &mut HashMap<String, Value>, (shift, ctrl, alt): (bool, bool, bool)) {
    e.insert("shiftkey".into(), shift.into());
    e.insert("ctrlkey".into(), ctrl.into());
    e.insert("altkey".into(), alt.into());
    e.insert("returnvalue".into(), true.into());
    e.insert("cancelbubble".into(), false.into());
}

/// A stored mediacenter value ("value|type").
pub fn restore(stored: &str) -> Value {
    match stored.rsplit_once('|') {
        Some((v, "b")) => (v == "true").into(),
        Some((v, "n")) => crate::value::parse_number(v).into(),
        Some((v, _)) => v.to_string().into(),
        None => stored.to_string().into(),
    }
}

fn store(v: &Value) -> String {
    let t = match v {
        Value::Bool(_) => "b",
        Value::Num(_) => "n",
        _ => "s",
    };
    format!("{}|{t}", v.to_str())
}

impl Skin {
    fn element_alive(&self, view: usize, element: usize) -> bool {
        self.views.get(view).and_then(Option::as_ref).is_some_and(|v| element < v.elements.len())
    }

    pub fn has_method(&self, o: ObjRef, name: &str) -> bool {
        match o {
            ObjRef::Element { view, element } => self.element_alive(view, element) && self.el_methods(self.el(view, element).kind).contains(&name),
            _ => members(o).1.contains(&name),
        }
    }

    pub fn has_member(&self, o: ObjRef, name: &str) -> bool {
        match o {
            ObjRef::Element { view, element } => {
                if !self.element_alive(view, element) {
                    return false;
                }
                let el = self.el(view, element);
                self.el_computed_names(el.kind).contains(&name) || el.values.contains_key(name) || element::is_declared(el.kind, name)
            }
            _ => members(o).0.contains(&name) || self.expandos.get(&o).is_some_and(|x| x.contains_key(name)),
        }
    }

    pub fn member_names(&self, o: ObjRef) -> Vec<String> {
        match o {
            ObjRef::Element { view, element } if self.element_alive(view, element) => {
                let el = self.el(view, element);
                self.el_computed_names(el.kind).iter().map(|s| s.to_string()).chain(el.values.keys().cloned()).collect()
            }
            ObjRef::Element { .. } => vec![],
            _ => members(o).0.iter().map(|s| s.to_string()).chain(self.expandos.get(&o).into_iter().flat_map(|x| x.keys().cloned())).collect(),
        }
    }

    pub fn get(&self, o: ObjRef, name: &str) -> Value {
        let name = name.to_lowercase();
        if let ObjRef::Element { view, element } = o {
            return if self.element_alive(view, element) { self.el_get(view, element, &name) } else { Value::Null };
        }
        let pm = &self.player;
        let m = &pm.media;
        let v: Value = match (o, name.as_str()) {
            (ObjRef::Player, "url") => pm.source_url().into(),
            (ObjRef::Player, "playstate") => pm.play_state.into(),
            (ObjRef::Player, "openstate") => pm.open_state.into(),
            (ObjRef::Player, "status") => pm.status.clone().into(),
            (ObjRef::Player, "currentmedia") | (ObjRef::Controls, "currentitem") => ObjRef::CurrentMedia.into(),
            (ObjRef::Player, "currentplaylist") => ObjRef::CurrentPlaylist.into(),
            (ObjRef::Player, "controls") => ObjRef::Controls.into(),
            (ObjRef::Player, "settings") => ObjRef::Settings.into(),
            (ObjRef::Player, "network") => ObjRef::Network.into(),
            (ObjRef::Player, "dvd") => ObjRef::Dvd.into(),
            (ObjRef::Player, "error") => ObjRef::Error.into(),
            (ObjRef::Player, "cdromcollection") => ObjRef::Cdroms.into(),
            (ObjRef::Player, "mediacollection") => ObjRef::MediaCollection.into(),
            (ObjRef::Player, "playlistcollection") => ObjRef::PlaylistCollection.into(),
            (ObjRef::Player, "versioninfo") => "12.0.26100.1".into(),
            (ObjRef::Player, "isonline" | "enabled" | "enablecontextmenu" | "stretchtofit") => true.into(),
            (ObjRef::Player, "isremote" | "fullscreen" | "windowlessvideo") => false.into(),
            (ObjRef::Player, "uimode") => "full".into(),
            (ObjRef::Player, "closedcaption" | "playerapplication") => Value::Null,

            (ObjRef::Controls, "currentposition") => pm.position.into(),
            (ObjRef::Controls, "currentpositionstring") => {
                if pm.duration() > 0.0 || pm.position > 0.0 {
                    format_time(pm.position).into()
                } else {
                    String::new().into()
                }
            }
            (ObjRef::Controls, "currentpositiontimecode") => format!("[00000]{}.00", format_time(pm.position)).into(),
            (ObjRef::Controls, _) => 0.0.into(),

            (ObjRef::Settings, "volume") => pm.volume().into(),
            (ObjRef::Settings, "mute") => pm.mute().into(),
            (ObjRef::Settings, "balance") => pm.balance().into(),
            (ObjRef::Settings, "shuffle") => pm.shuffle().into(),
            (ObjRef::Settings, "loop") => pm.repeat().into(),
            (ObjRef::Settings, "rate" | "playcount") => 1.0.into(),
            (ObjRef::Settings, "autostart" | "invokeurls") => true.into(),
            (ObjRef::Settings, "enableerrordialogs") => false.into(),
            (ObjRef::Settings, "mediaaccessrights") => "full".into(),
            (ObjRef::Settings, _) => String::new().into(),

            (ObjRef::CurrentMedia, "name") => m.title.clone().into(),
            (ObjRef::CurrentMedia, "duration") => pm.duration().into(),
            (ObjRef::CurrentMedia, "durationstring") => if pm.duration() > 0.0 { format_time(pm.duration()) } else { String::new() }.into(),
            (ObjRef::CurrentMedia, "sourceurl") => pm.source_url().into(),
            (ObjRef::CurrentMedia, "attributecount") => MEDIA_ATTRIBUTES.len().into(),
            (ObjRef::CurrentMedia, _) => 0.0.into(),

            (ObjRef::HistoryItem(i), _) => match self.history.get(i) {
                Some(t) => match name.as_str() {
                    "name" => t.title.clone().into(),
                    "duration" => t.duration.into(),
                    "durationstring" => format_time(t.duration).into(),
                    _ => String::new().into(),
                },
                None => Value::Null,
            },

            (ObjRef::CurrentPlaylist, "count") => self.history.len().max(1).into(),
            (ObjRef::CurrentPlaylist, "name") => if m.source_app.is_empty() { "Now Playing".to_string() } else { m.source_app.clone() }.into(),
            (ObjRef::CurrentPlaylist, "attributecount") => 0.0.into(),

            (ObjRef::Network, "bufferingprogress" | "downloadprogress" | "receptionquality") => 100.0.into(),
            (ObjRef::Network, "sourceprotocol") => "https".into(),
            (ObjRef::Network, _) => 0.0.into(),
            (ObjRef::Dvd, "domain") => String::new().into(),
            (ObjRef::Error, "errorcount") | (ObjRef::Cdroms, "count") => 0.0.into(),

            (ObjRef::MediaCenter, "effecttype" | "effectpreset" | "effectpresettitle" | "effecttitle") => self.mediacenter_vis(&name),
            (ObjRef::MediaCenter, "videozoom") => self.expando(o, &name).unwrap_or(100.0.into()),
            (ObjRef::MediaCenter, "videostretchtofit" | "showeffects" | "showtitles") => self.expando(o, &name).unwrap_or(true.into()),
            (ObjRef::MediaCenter, "contrastmode") => false.into(),

            (ObjRef::Theme, "currentviewid") => self.active_view.and_then(|v| self.view(v).id.clone()).unwrap_or_default().into(),
            (ObjRef::Theme, "title") => self.title.clone().into(),
            (ObjRef::Theme, "author") => self.author.clone().into(),
            (ObjRef::Theme, "copyright") => self.copyright.clone().into(),
            (ObjRef::Theme, "version") => "12.0".into(),

            (ObjRef::Event, "screenwidth") => self.work_area.w.into(),
            (ObjRef::Event, "screenheight") => self.work_area.h.into(),
            (ObjRef::Event, "screenleft") => self.work_area.x.into(),
            (ObjRef::Event, "screentop") => self.work_area.y.into(),
            (ObjRef::Event, "fullscreenwidth") => self.screen.0.into(),
            (ObjRef::Event, "fullscreenheight") => self.screen.1.into(),
            (ObjRef::Event, n) => self.event.get(n).cloned().unwrap_or_default(),

            _ => self.expando(o, &name).unwrap_or_default(),
        };
        v
    }

    fn expando(&self, o: ObjRef, name: &str) -> Option<Value> {
        self.expandos.get(&o).and_then(|x| x.get(name)).cloned()
    }

    pub fn set(&mut self, o: ObjRef, name: &str, value: Value) {
        let name = name.to_lowercase();
        match (o, name.as_str()) {
            (ObjRef::Element { view, element }, _) => {
                if !self.element_alive(view, element) {
                    return;
                }
                if element == 0 && matches!(name.as_str(), "left" | "top") {
                    let n = value.to_number();
                    let v = self.view_mut(view);
                    if name == "left" {
                        v.left = n
                    } else {
                        v.top = n
                    }
                    self.host_call(move |h| h.view_moved(view));
                    return;
                }
                if self.el_computed_names(self.el(view, element).kind).contains(&name.as_str()) {
                    return;
                }
                self.set_value(view, element, &name, value, true);
            }
            (ObjRef::Player, "url") => {
                let url = value.to_str();
                if url.to_lowercase().starts_with("http") {
                    self.host_call(move |h| h.open_url(&url));
                }
            }
            (ObjRef::Player, "fullscreen") => {}
            (ObjRef::Controls, "currentposition") => {
                self.player.set_position(value.to_number());
                self.flush_player();
            }
            (ObjRef::Settings, "volume") => {
                self.player.set_volume(value.to_number().round() as i32);
                self.flush_player();
            }
            (ObjRef::Settings, "mute") => {
                self.player.set_mute(value.to_bool());
                self.flush_player();
            }
            (ObjRef::Settings, "balance") => {
                self.player.set_balance(value.to_number() as i32);
                self.flush_player();
            }
            (ObjRef::Settings, "shuffle") => {
                self.player.set_shuffle(value.to_bool());
                self.flush_player();
            }
            (ObjRef::Settings, "loop") => {
                self.player.set_repeat(value.to_bool());
                self.flush_player();
            }
            (ObjRef::Theme, "currentviewid") => self.deferred.push(Deferred::SwitchView(value.to_str())),
            (ObjRef::Event, n) => {
                self.event.insert(n.to_string(), value);
            }
            (ObjRef::MediaCenter, "effecttype" | "effectpreset" | "effectpresettitle" | "effecttitle") => self.set_mediacenter_vis(&name, &value),
            (ObjRef::MediaCenter, "contrastmode") => {}
            (ObjRef::MediaCenter, _) => {
                if !self.preview {
                    self.settings.media_center.insert(name.clone(), store(&value));
                }
                let changed = self.expando(o, &name).is_none_or(|old| old != value);
                self.expandos.entry(o).or_default().insert(name.clone(), value);
                if changed {
                    self.notify(o, &name);
                }
            }
            // Read-only WMP properties ignore writes; anything else is an expando.
            _ if members(o).0.contains(&name.as_str()) => {}
            _ => {
                self.expandos.entry(o).or_default().insert(name.clone(), value);
                self.notify(o, &name);
            }
        }
    }

    fn flush_player(&mut self) {
        let changes = self.player.take_changes();
        self.apply_player_changes(changes);
    }

    pub fn call(&mut self, o: ObjRef, name: &str, a: &[Value]) -> Value {
        let name = name.to_lowercase();
        let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
        let text = |i: usize| arg(i).to_str();
        if let ObjRef::Element { view, element } = o {
            if !self.element_alive(view, element) {
                return Value::Null;
            }
            return self.element_call(view, element, &name, a).unwrap_or_default();
        }
        let r: Value = match (o, name.as_str()) {
            (ObjRef::Player, "launchurl") => {
                let url = text(0);
                self.host_call(move |h| h.open_url(&url));
                Value::Null
            }
            (ObjRef::Player, "close") => {
                self.deferred.push(Deferred::Close);
                Value::Null
            }
            (ObjRef::Player, "newmedia") => ObjRef::CurrentMedia.into(),
            (ObjRef::Player, "newplaylist") => ObjRef::CurrentPlaylist.into(),
            (ObjRef::Player, "openplayer") => {
                self.host_call(|h| h.choose_skin());
                Value::Null
            }

            (ObjRef::Controls, "play" | "playitem") => self.transport(|p| p.play()),
            (ObjRef::Controls, "pause") => self.transport(|p| p.pause()),
            (ObjRef::Controls, "stop") => self.transport(|p| p.stop()),
            (ObjRef::Controls, "next") => self.transport(|p| p.next()),
            (ObjRef::Controls, "previous") => self.transport(|p| p.previous()),
            (ObjRef::Controls, "fastforward") => self.transport(|p| p.fast_forward()),
            (ObjRef::Controls, "fastreverse") => self.transport(|p| p.fast_reverse()),
            (ObjRef::Controls, "isavailable") => self.player.is_available(&text(0)).into(),
            (ObjRef::Controls, _) => Value::Null,

            (ObjRef::Settings, "getmode") => match text(0).to_lowercase().as_str() {
                "shuffle" => self.player.shuffle().into(),
                "loop" => self.player.repeat().into(),
                "autorewind" => true.into(),
                _ => false.into(),
            },
            (ObjRef::Settings, "setmode") => {
                match text(0).to_lowercase().as_str() {
                    "shuffle" => self.player.set_shuffle(arg(1).to_bool()),
                    "loop" => self.player.set_repeat(arg(1).to_bool()),
                    _ => {}
                }
                self.flush_player();
                Value::Null
            }
            (ObjRef::Settings, "isavailable" | "requestmediaaccessrights") => true.into(),

            (ObjRef::CurrentMedia, "getiteminfo" | "getiteminfobytype") => self.player.item_info(&text(0)).into(),
            (ObjRef::CurrentMedia, "getiteminfobyatom") => {
                let attr = MEDIA_ATTRIBUTES.get(arg(0).to_number().max(0.0) as usize).copied().unwrap_or("");
                self.player.item_info(attr).into()
            }
            (ObjRef::CurrentMedia, "getattributename") => MEDIA_ATTRIBUTES.get(arg(0).to_number().max(0.0) as usize).copied().unwrap_or("").into(),
            (ObjRef::CurrentMedia, "getattributecountbytype" | "getmarkertime") => 0.0.into(),
            (ObjRef::CurrentMedia, "getmarkername") => String::new().into(),
            (ObjRef::CurrentMedia, "isidentical") => (arg(0) == Value::Obj(ObjRef::CurrentMedia)).into(),
            (ObjRef::CurrentMedia, "ismemberof" | "isreadonlyitem") => true.into(),
            (ObjRef::CurrentMedia, _) => Value::Null,

            (ObjRef::HistoryItem(i), "getiteminfo") => match self.history.get(i) {
                Some(t) => match text(0).to_lowercase().as_str() {
                    "title" | "name" => t.title.clone().into(),
                    "author" | "artist" | "wm/albumartist" => t.artist.clone().into(),
                    "album" | "wm/albumtitle" => t.album.clone().into(),
                    _ => String::new().into(),
                },
                None => String::new().into(),
            },
            (ObjRef::HistoryItem(_), _) => false.into(),

            (ObjRef::CurrentPlaylist, "item") => {
                let i = arg(0).to_number().max(0.0) as usize;
                if self.history.is_empty() || i + 1 == self.history.len() {
                    ObjRef::CurrentMedia.into()
                } else if i < self.history.len() {
                    ObjRef::HistoryItem(i).into()
                } else {
                    Value::Null
                }
            }
            (ObjRef::CurrentPlaylist, "getiteminfo" | "getattributename") => String::new().into(),
            (ObjRef::CurrentPlaylist, "isidentical") => (arg(0) == Value::Obj(ObjRef::CurrentPlaylist)).into(),
            (ObjRef::CurrentPlaylist, _) => Value::Null,

            (ObjRef::Network, "getproxyname") => String::new().into(),
            (ObjRef::Network, "getproxysettings") => 0.0.into(),
            (ObjRef::Dvd, "isavailable") => false.into(),

            (ObjRef::Theme, "openview") => {
                self.deferred.push(Deferred::OpenView { id: text(0), at: None });
                Value::Null
            }
            (ObjRef::Theme, "openviewrelative") => {
                self.deferred.push(Deferred::OpenView { id: text(0), at: Some((arg(1).to_number(), arg(2).to_number())) });
                Value::Null
            }
            (ObjRef::Theme, "closeview") => {
                let id = text(0);
                let target = self.open_views().find(|&v| self.view(v).id.as_deref().is_some_and(|i| i.eq_ignore_ascii_case(&id)));
                if let Some(v) = target {
                    self.deferred.push(Deferred::CloseView(v));
                }
                Value::Null
            }
            (ObjRef::Theme, "loadpreference") => self.load_preference(&text(0)).into(),
            (ObjRef::Theme, "savepreference") => {
                self.save_preference(&text(0), text(1));
                Value::Null
            }
            (ObjRef::Theme, "loadstring") => self.load_string(&text(0)).into(),
            (ObjRef::Theme, "opendialog") => {
                let t = text(0).to_uppercase();
                if matches!(t.as_str(), "FILE_OPEN" | "URL_OPEN" | "OPEN" | "SKIN_CHOOSER" | "SKINS") {
                    self.host_call(|h| h.choose_skin());
                } else {
                    crate::log(&format!("theme.openDialog('{t}') is not supported"));
                }
                Value::Null
            }
            (ObjRef::Theme, "playsound") => {
                self.play_sound(&text(0));
                Value::Null
            }
            (ObjRef::Theme, "logstring") => {
                crate::log(&format!("[skin] {}", text(0)));
                Value::Null
            }
            _ => Value::Null,
        };
        r
    }

    fn transport(&mut self, f: impl FnOnce(&mut crate::player::PlayerModel)) -> Value {
        f(&mut self.player);
        self.flush_player();
        Value::Null
    }
}
