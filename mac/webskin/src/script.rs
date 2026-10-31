//! One JavaScript context per skin VIEW (as WMP had one JScript context per view), whose WMP
//! objects are case-insensitive proxies over the Rust object model in [`crate::skin`].

use crate::skin::{self, Shared};
use crate::value::{ObjRef, Value};
use regex::Regex;
use rquickjs::context::EvalOptions;
use rquickjs::function::Rest;
use rquickjs::{Context, Ctx, Function, Object, Persistent, Runtime};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// A script value kept on the Rust side: a function stored as a handler, or an array/object a
/// script hung on an element as an expando.
#[derive(Clone)]
pub struct JsRef {
    value: Rc<Persistent<rquickjs::Value<'static>>>,
    pub is_function: bool,
}

impl PartialEq for JsRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.value, &other.value)
    }
}

impl std::fmt::Debug for JsRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.is_function { "JsRef(function)" } else { "JsRef(object)" })
    }
}

thread_local! {
    static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
    /// How many script calls are running: WMP ran *_onchange handlers synchronously, so a script that
    /// sets a property runs the handler while the runtime is already locked by the outer call.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

const SCRIPT_TIMEOUT: Duration = Duration::from_secs(4);

/// The runtime all views share, with WMP's limits on runaway scripts.
pub fn new_runtime() -> Runtime {
    let rt = Runtime::new().expect("JavaScript runtime");
    rt.set_max_stack_size(1024 * 1024);
    rt.set_interrupt_handler(Some(Box::new(|| DEADLINE.with(|d| d.get().is_some_and(|t| Instant::now() > t)))));
    rt
}

/// Runs `f` under the script time limit; nested calls share the outermost deadline.
fn timed<T>(f: impl FnOnce() -> T) -> T {
    let outer = DEADLINE.with(|d| d.get());
    if outer.is_none() {
        DEADLINE.with(|d| d.set(Some(Instant::now() + SCRIPT_TIMEOUT)));
    }
    let r = f();
    if outer.is_none() {
        DEADLINE.with(|d| d.set(None));
    }
    r
}

/// Handles: shared WMP objects are small numbers, history items 500+, elements 1000+.
pub fn encode(o: ObjRef) -> f64 {
    let n = match o {
        ObjRef::Player => 1,
        ObjRef::Controls => 2,
        ObjRef::Settings => 3,
        ObjRef::CurrentMedia => 4,
        ObjRef::Network => 5,
        ObjRef::CurrentPlaylist => 6,
        ObjRef::Dvd => 7,
        ObjRef::Error => 8,
        ObjRef::Cdroms => 9,
        ObjRef::MediaCollection => 10,
        ObjRef::PlaylistCollection => 11,
        ObjRef::MediaCenter => 12,
        ObjRef::Theme => 13,
        ObjRef::Event => 14,
        ObjRef::HistoryItem(i) => 500 + i as u64,
        ObjRef::Element { view, element } => 1000 + (view as u64) * 65536 + element as u64,
    };
    n as f64
}

pub fn decode(h: f64) -> Option<ObjRef> {
    let n = h as u64;
    Some(match n {
        1 => ObjRef::Player,
        2 => ObjRef::Controls,
        3 => ObjRef::Settings,
        4 => ObjRef::CurrentMedia,
        5 => ObjRef::Network,
        6 => ObjRef::CurrentPlaylist,
        7 => ObjRef::Dvd,
        8 => ObjRef::Error,
        9 => ObjRef::Cdroms,
        10 => ObjRef::MediaCollection,
        11 => ObjRef::PlaylistCollection,
        12 => ObjRef::MediaCenter,
        13 => ObjRef::Theme,
        14 => ObjRef::Event,
        500..1000 => ObjRef::HistoryItem((n - 500) as usize),
        1000.. => ObjRef::Element { view: ((n - 1000) / 65536) as usize, element: ((n - 1000) % 65536) as usize },
        _ => return None,
    })
}

const PRELUDE: &str = r#"
var __proxies = {};
var __methods = {};
function __wrap(h) {
    var p = __proxies[h];
    if (p) return p;
    p = new Proxy({}, {
        has: function (t, k) { return typeof k === 'string' && __kind(h, k) !== 0; },
        get: function (t, k) {
            if (k === '__h') return h;
            if (typeof k !== 'string') return undefined;
            var kind = __kind(h, k);
            if (kind === 2) {
                var key = h + '|' + k.toLowerCase();
                var m = __methods[key];
                if (!m) { m = function () { return __call(h, k, Array.prototype.slice.call(arguments)); }; __methods[key] = m; }
                return m;
            }
            return kind === 1 ? __get(h, k) : undefined;
        },
        set: function (t, k, v) { if (typeof k === 'string') __set(h, k, v); return true; },
        deleteProperty: function () { return true; },
        ownKeys: function () { return __keys(h); },
        getOwnPropertyDescriptor: function (t, k) {
            if (typeof k !== 'string' || __kind(h, k) === 0) return undefined;
            return { value: this.get(t, k), writable: true, enumerable: true, configurable: true };
        }
    });
    __proxies[h] = p;
    return p;
}
var __evalIn = function (__self, __code) { with (__self) { return eval(__code); } };
function ScriptEngine() { return 'JScript'; }
function ScriptEngineMajorVersion() { return 5; }
function ScriptEngineMinorVersion() { return 8; }
function ScriptEngineBuildVersion() { return 1; }
function Enumerator(c) { this._c = c; this._i = 0; }
Enumerator.prototype.atEnd = function () { var n = this._c ? (this._c.count !== undefined ? this._c.count : this._c.length) : 0; return this._i >= n; };
Enumerator.prototype.item = function () { return this._c.item ? this._c.item(this._i) : this._c[this._i]; };
Enumerator.prototype.moveNext = function () { this._i++; };
Enumerator.prototype.moveFirst = function () { this._i = 0; };
function ActiveXObject(n) { throw new Error('ActiveXObject is not available: ' + n); }
function GetObject() { return null; }
function __assignToCall() { throw new ReferenceError('Cannot assign to a function result'); }
"#;

fn sloppy() -> EvalOptions {
    let mut o = EvalOptions::default();
    o.strict = false;
    o
}

pub struct ScriptHost {
    pub ctx: Context,
    label: String,
    /// Errors already logged: a handler that fails on every position change logs once.
    logged: RefCell<HashSet<String>>,
}

impl ScriptHost {
    /// Runs `f` in this view's context: locking the runtime, or inside a running script reusing
    /// the lock that script holds.
    fn enter<R>(&self, f: impl for<'js> FnOnce(Ctx<'js>) -> R) -> R {
        DEPTH.with(|d| d.set(d.get() + 1));
        let nested = DEPTH.with(|d| d.get()) > 1;
        // SAFETY: nested calls only happen from host functions called by a script, which run while
        // the outer `Context::with` holds the runtime lock; contexts share that runtime.
        let r = if nested { f(unsafe { Ctx::from_raw(self.ctx.as_raw()) }) } else { self.ctx.with(f) };
        DEPTH.with(|d| d.set(d.get() - 1));
        r
    }

    pub fn new(rt: &Runtime, label: String, shared: Weak<Shared>) -> Self {
        let ctx = Context::full(rt).expect("JavaScript context");
        ctx.with(|ctx| install(&ctx, shared));
        Self { ctx, label, logged: RefCell::default() }
    }

    fn report(&self, ctx: &Ctx<'_>, place: &str, err: rquickjs::Error) -> String {
        let msg = match err {
            rquickjs::Error::Exception => {
                let e = ctx.catch();
                match e.as_exception() {
                    Some(ex) => {
                        let line = ex.stack().and_then(|s| s.lines().next().map(str::trim).map(str::to_string)).unwrap_or_default();
                        format!("{} {line}", ex.message().unwrap_or_default())
                    }
                    None => e.as_string().and_then(|s| s.to_string().ok()).unwrap_or_else(|| format!("{e:?}")),
                }
            }
            other => other.to_string(),
        };
        let line = format!("[{}] script error in {place}: {}", self.label, msg.trim());
        if self.logged.borrow_mut().insert(line.clone()) {
            crate::log(&line);
        }
        line
    }

    pub fn set_global(&self, name: &str, value: &Value) {
        self.enter(|ctx| {
            if let Ok(v) = to_js(&ctx, value) {
                let _ = ctx.globals().set(name, v);
            }
        });
    }

    pub fn get_global(&self, name: &str) -> Value {
        self.enter(|ctx| ctx.globals().get::<_, rquickjs::Value>(name).map(|v| from_js(&ctx, v)).unwrap_or_default())
    }

    /// Runs a script file in the global scope.
    pub fn execute(&self, code: &str, source: &str) -> Result<(), String> {
        let code = prepare(code);
        timed(|| self.enter(|ctx| ctx.eval_with_options::<(), _>(code.as_str(), sloppy()).map_err(|e| self.report(&ctx, source, e))))
    }

    /// Compiles handler code into `function(scope0.., params..) { with(scope0) ... { code } }`.
    pub fn compile(&self, code: &str, name: &str, scopes: usize, params: &[&str]) -> Result<Option<JsRef>, String> {
        let code = strip_prefix(code);
        if code.trim().is_empty() {
            return Ok(None);
        }
        let scope_names: Vec<String> = (0..scopes).map(|i| format!("__s{i}")).collect();
        let withs: String = scope_names.iter().map(|s| format!("with({s})")).collect();
        let all: Vec<String> = scope_names.iter().cloned().chain(params.iter().map(|p| p.to_string())).collect();
        let src = format!("(function({}){{ {withs}{{ {}\n}} }})", all.join(","), prepare(code));
        self.enter(|ctx| match ctx.eval_with_options::<rquickjs::Value, _>(src.as_str(), sloppy()) {
            Ok(v) => Ok(Some(save(&ctx, v))),
            Err(e) => Err(self.report(&ctx, &format!("{name} (compile)"), e)),
        })
    }

    /// Calls a function with `this` set to `this_obj` (when given).
    pub fn call(&self, f: &JsRef, this_obj: Option<ObjRef>, args: &[Value], place: &str) -> Result<Value, String> {
        if !f.is_function {
            return Ok(Value::Null);
        }
        timed(|| {
            self.enter(|ctx| {
                let run = || -> rquickjs::Result<Value> {
                    let func: Function = Persistent::clone(&f.value).restore(&ctx)?.get()?;
                    let this = match this_obj {
                        Some(o) => to_js(&ctx, &Value::Obj(o))?,
                        None => rquickjs::Value::new_undefined(ctx.clone()),
                    };
                    let mut a = rquickjs::function::Args::new(ctx.clone(), args.len());
                    a.this(this)?;
                    for v in args {
                        a.push_arg(to_js(&ctx, v)?)?;
                    }
                    let r: rquickjs::Value = func.call_arg(a)?;
                    Ok(from_js(&ctx, r))
                };
                run().map_err(|e| self.report(&ctx, place, e))
            })
        })
    }

    /// Evaluates a "jscript:" attribute in an element's scope.
    pub fn eval_in(&self, scope: ObjRef, expression: &str, place: &str) -> Result<Value, String> {
        let code = prepare(expression);
        timed(|| {
            self.enter(|ctx| {
                let run = || -> rquickjs::Result<Value> {
                    let f: Function = ctx.globals().get("__evalIn")?;
                    let r: rquickjs::Value = f.call((to_js(&ctx, &Value::Obj(scope))?, code.as_str()))?;
                    Ok(from_js(&ctx, r))
                };
                run().map_err(|e| self.report(&ctx, place, e))
            })
        })
    }

    /// WMP resolved global names case-insensitively (a skin calling UpdateMetaData() when it defines
    /// UpdateMetadata()). For each identifier the skin's code uses that differs only by case from a
    /// defined global, add an alias property that forwards to the real one.
    pub fn add_case_aliases(&self, sources: &[String]) {
        self.enter(|ctx| {
            let Ok(names) = ctx.eval_with_options::<Vec<String>, _>("Object.getOwnPropertyNames(globalThis)", sloppy()) else { return };
            let exact: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();
            let mut by_lower = std::collections::HashMap::new();
            for n in &names {
                by_lower.entry(n.to_lowercase()).or_insert(n.as_str());
            }
            let Ok(alias) = ctx.eval_with_options::<Function, _>(
                "(function(a,t){ Object.defineProperty(globalThis,a,{get:function(){return globalThis[t];},set:function(v){globalThis[t]=v;},configurable:true}); })",
                sloppy(),
            ) else {
                return;
            };
            let mut done = std::collections::HashSet::new();
            for src in sources {
                for id in identifiers(src) {
                    if exact.contains(id) || !done.insert(id.to_string()) {
                        continue;
                    }
                    if let Some(target) = by_lower.get(&id.to_lowercase()).filter(|t| **t != id) {
                        let _ = alias.call::<_, ()>((id, *target));
                    }
                }
            }
        });
    }
}

/// The host functions the proxies in [`PRELUDE`] call, then the prelude itself.
fn install<'js>(ctx: &Ctx<'js>, shared: Weak<Shared>) {
    let g = ctx.globals();
    let w = shared.clone();
    let kind = Function::new(ctx.clone(), move |h: f64, key: String| -> i32 {
        let (Some(s), Some(o)) = (w.upgrade(), decode(h)) else { return 0 };
        skin::bridge_kind(&s, o, &key)
    });
    let w = shared.clone();
    let get = Function::new(ctx.clone(), move |ctx: Ctx<'js>, h: f64, key: String| -> rquickjs::Result<rquickjs::Value<'js>> {
        let v = match (w.upgrade(), decode(h)) {
            (Some(s), Some(o)) => skin::bridge_get(&s, o, &key),
            _ => Value::Null,
        };
        to_js(&ctx, &v)
    });
    let w = shared.clone();
    let set = Function::new(ctx.clone(), move |ctx: Ctx<'js>, h: f64, key: String, v: rquickjs::Value<'js>| {
        if let (Some(s), Some(o)) = (w.upgrade(), decode(h)) {
            let v = from_js(&ctx, v);
            skin::bridge_set(&s, o, &key, v);
        }
    });
    let w = shared.clone();
    let call = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, h: f64, key: String, args: Rest<rquickjs::Value<'js>>| -> rquickjs::Result<rquickjs::Value<'js>> {
            let args: Vec<Value> = match args.0.into_iter().next().and_then(|a| a.into_array()) {
                Some(arr) => arr.iter::<rquickjs::Value>().filter_map(Result::ok).map(|v| from_js(&ctx, v)).collect(),
                None => vec![],
            };
            let v = match (w.upgrade(), decode(h)) {
                (Some(s), Some(o)) => skin::bridge_call(&s, o, &key, args),
                _ => Value::Null,
            };
            to_js(&ctx, &v)
        },
    );
    let w = shared;
    let keys = Function::new(ctx.clone(), move |h: f64| -> Vec<String> {
        match (w.upgrade(), decode(h)) {
            (Some(s), Some(o)) => skin::bridge_keys(&s, o),
            _ => vec![],
        }
    });
    for (name, f) in [("__kind", kind), ("__get", get), ("__set", set), ("__call", call), ("__keys", keys)] {
        g.set(name, f.expect("host function")).expect("set host function");
    }
    ctx.eval_with_options::<(), _>(PRELUDE, sloppy()).expect("prelude");
}

fn save<'js>(ctx: &Ctx<'js>, v: rquickjs::Value<'js>) -> JsRef {
    let is_function = v.is_function();
    JsRef { value: Rc::new(Persistent::save(ctx, v)), is_function }
}

pub fn to_js<'js>(ctx: &Ctx<'js>, v: &Value) -> rquickjs::Result<rquickjs::Value<'js>> {
    Ok(match v {
        Value::Null => rquickjs::Value::new_null(ctx.clone()),
        Value::Bool(b) => rquickjs::Value::new_bool(ctx.clone(), *b),
        Value::Num(n) => rquickjs::Value::new_number(ctx.clone(), *n),
        Value::Str(s) => rquickjs::String::from_str(ctx.clone(), s)?.into_value(),
        Value::Obj(o) => {
            let wrap: Function = ctx.globals().get("__wrap")?;
            wrap.call((encode(*o),))?
        }
        Value::Js(r) => Persistent::clone(&r.value).restore(ctx)?,
    })
}

pub fn from_js<'js>(ctx: &Ctx<'js>, v: rquickjs::Value<'js>) -> Value {
    if v.is_undefined() || v.is_null() {
        return Value::Null;
    }
    if let Some(b) = v.as_bool() {
        return Value::Bool(b);
    }
    if let Some(n) = v.as_number() {
        return Value::Num(n);
    }
    if let Some(s) = v.as_string() {
        return Value::Str(s.to_string().unwrap_or_default());
    }
    if let Some(obj) = v.as_object().filter(|_| !v.is_function())
        && let Ok(Some(h)) = Object::get::<_, Option<f64>>(obj, "__h")
        && let Some(o) = decode(h)
    {
        return Value::Obj(o);
    }
    Value::Js(save(ctx, v))
}

/// Removes an optional "jscript:" / "javascript:" prefix from handler code.
pub fn strip_prefix(code: &str) -> &str {
    let code = code.trim();
    for p in ["jscript:", "javascript:"] {
        if code.len() >= p.len() && code[..p.len()].eq_ignore_ascii_case(p) {
            return code[p.len()..].trim();
        }
    }
    code
}

/// WMP object properties. JScript invoked COM members with DISPATCH_METHOD|DISPATCH_PROPERTYGET, so
/// skins could write "mediacenter.effectType()" and get the property value. None of these names is
/// a method anywhere in WMP's object model.
const PROPERTY_NAMES: &[&str] = &[
    "effecttype",
    "effectpreset",
    "effecttitle",
    "effectpresettitle",
    "showeffects",
    "showtitles",
    "videozoom",
    "videostretchtofit",
    "url",
    "playstate",
    "openstate",
    "status",
    "currentmedia",
    "currentplaylist",
    "controls",
    "settings",
    "network",
    "versioninfo",
    "fullscreen",
    "uimode",
    "isonline",
    "currentposition",
    "currentpositionstring",
    "currentpositiontimecode",
    "currentitem",
    "volume",
    "mute",
    "balance",
    "rate",
    "autostart",
    "playcount",
    "baseurl",
    "duration",
    "durationstring",
    "sourceurl",
    "imagesourcewidth",
    "imagesourceheight",
    "attributecount",
    "markercount",
    "bufferingprogress",
    "downloadprogress",
    "receptionquality",
    "bitrate",
    "bandwidth",
    "currentviewid",
    "currenteffecttype",
    "currentpreset",
    "currenteffecttitle",
    "currentpresettitle",
    "gainlevel1",
    "gainlevel2",
    "gainlevel3",
    "gainlevel4",
    "gainlevel5",
    "gainlevel6",
    "gainlevel7",
    "gainlevel8",
    "gainlevel9",
    "gainlevel10",
    "selecteditem",
    "itemcount",
    "focusobjectid",
    "timerinterval",
];

/// Smooths over the differences between Microsoft's JScript and a standards-based engine.
pub fn prepare(code: &str) -> String {
    static EVENT_FUNCTION: OnceLock<Regex> = OnceLock::new();
    static PROPERTY_CALL: OnceLock<Regex> = OnceLock::new();
    static INDEXED_PUT: OnceLock<Regex> = OnceLock::new();
    static CALL_ASSIGNMENT: OnceLock<Regex> = OnceLock::new();
    let mut code = code.to_string();
    if code.contains("gainLevels") {
        // JScript assigned COM indexed properties: "eq.gainLevels(band) = value".
        let re = INDEXED_PUT.get_or_init(|| Regex::new(r"\.gainLevels\s*\(([^()]*)\)\s*=([^=][^;\r\n]*)").expect("regex"));
        code = re.replace_all(&code, ".gainLevels($1, $2)").into_owned();
    }
    if code.contains(")") {
        // JScript compiled any other assignment to a call ("theme.savePreference('x') = 1") and
        // threw only when it ran; a standard engine rejects the whole file.
        let re =
            CALL_ASSIGNMENT.get_or_init(|| Regex::new(r"([A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*\([^()\r\n]*\))\s*=([^=][^;\r\n]*)").expect("regex"));
        code = re.replace_all(&code, "($1, $2, __assignToCall())").into_owned();
    }
    if code.contains("::") {
        // JScript's "function obj::event(...)" event-sink syntax.
        let re = EVENT_FUNCTION.get_or_init(|| Regex::new(r"function\s+([A-Za-z_$][\w$]*)::([A-Za-z_$][\w$]*)\s*\(").expect("regex"));
        code = re.replace_all(&code, "function ${1}__${2}(").into_owned();
    }
    if code.contains("()") {
        let re = PROPERTY_CALL.get_or_init(|| Regex::new(r"\.([A-Za-z_]\w*)\s*\(\s*\)").expect("regex"));
        code = re
            .replace_all(
                &code,
                |c: &regex::Captures| {
                    if PROPERTY_NAMES.contains(&c[1].to_lowercase().as_str()) { format!(".{}", &c[1]) } else { c[0].to_string() }
                },
            )
            .into_owned();
    }
    code
}

/// Identifiers in code that aren't member accesses (not preceded by a word character, `$` or `.`).
fn identifiers(src: &str) -> impl Iterator<Item = &str> {
    let bytes = src.as_bytes();
    let is_start = |b: u8| b.is_ascii_alphabetic() || b == b'_' || b == b'$';
    let is_part = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < bytes.len() {
            if is_start(bytes[i]) && (i == 0 || !(is_part(bytes[i - 1]) || bytes[i - 1] == b'.')) {
                let start = i;
                while i < bytes.len() && is_part(bytes[i]) {
                    i += 1;
                }
                return Some(&src[start..i]);
            }
            i += 1;
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepares_jscript_quirks() {
        assert_eq!(prepare("x = mediacenter.effectType();"), "x = mediacenter.effectType;");
        assert_eq!(prepare("player.controls.play();"), "player.controls.play();");
        assert_eq!(prepare("function player::playStateChange(s) {}"), "function player__playStateChange(s) {}");
        assert_eq!(strip_prefix(" JScript: a()"), "a()");
        assert_eq!(prepare("eq.gainLevels(band) = value;"), "eq.gainLevels(band,  value);");
        assert_eq!(prepare("x = eq.gainLevels(2) == 3;"), "x = eq.gainLevels(2) == 3;");
        assert_eq!(prepare("theme.savePreference('a')='--';"), "(theme.savePreference('a'), '--', __assignToCall());");
        assert_eq!(prepare("if (f(1) == 2 || g(3) >= 4) x = h(5);"), "if (f(1) == 2 || g(3) >= 4) x = h(5);");
    }

    #[test]
    fn finds_free_identifiers() {
        let ids: Vec<&str> = identifiers("UpdateMetaData(); player.controls; a1 = $b").collect();
        assert_eq!(ids, vec!["UpdateMetaData", "player", "a1", "$b"]);
    }

    #[test]
    fn handles_round_trip() {
        for o in [ObjRef::Player, ObjRef::Event, ObjRef::HistoryItem(7), ObjRef::Element { view: 2, element: 300 }] {
            assert_eq!(decode(encode(o)), Some(o));
        }
    }
}
