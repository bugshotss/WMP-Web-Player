//! Values that skin scripts and skin attributes pass around, with WMP's loose coercions.

use crate::script::JsRef;

/// What a WMP object property holds. Object references are handles into the skin's object model.
#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Obj(ObjRef),
    /// A script function or object stored on a WMP object (`button.onclick = function () {...}`).
    Js(JsRef),
}

/// A scriptable object in a skin: a view's element, or one of WMP's shared objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjRef {
    Element {
        view: usize,
        element: usize,
    },
    Player,
    Controls,
    Settings,
    CurrentMedia,
    Network,
    CurrentPlaylist,
    Dvd,
    Error,
    Cdroms,
    MediaCollection,
    PlaylistCollection,
    MediaCenter,
    Theme,
    Event,
    /// An earlier track in `player.currentPlaylist`, by history index.
    HistoryItem(usize),
}

/// The declared type of a property: values are coerced to it when stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PType {
    Any,
    Bool,
    Int,
    Number,
    String,
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a == b || (a.is_nan() && b.is_nan()),
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Obj(a), Value::Obj(b)) => a == b,
            (Value::Js(a), Value::Js(b)) => a == b,
            _ => false,
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Num(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Num(v as f64)
    }
}
impl From<usize> for Value {
    fn from(v: usize) -> Self {
        Value::Num(v as f64)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}
impl From<ObjRef> for Value {
    fn from(v: ObjRef) -> Self {
        Value::Obj(v)
    }
}

impl Value {
    pub fn coerce(self, t: PType) -> Value {
        match t {
            PType::Bool => Value::Bool(self.to_bool()),
            PType::Int => Value::Num(self.to_number().trunc()),
            PType::Number => Value::Num(self.to_number()),
            PType::String => Value::Str(self.to_str()),
            PType::Any => self,
        }
    }

    pub fn to_bool(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(d) => *d != 0.0 && !d.is_nan(),
            Value::Str(s) => parse_bool(s),
            Value::Obj(_) | Value::Js(_) => true,
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            Value::Null | Value::Obj(_) | Value::Js(_) => 0.0,
            Value::Num(d) if d.is_nan() => 0.0,
            Value::Num(d) => *d,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Str(s) => parse_number(s),
        }
    }

    pub fn to_int(&self) -> i32 {
        let n = self.to_number();
        n.clamp(i32::MIN as f64, i32::MAX as f64) as i32
    }

    pub fn to_str(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Str(s) => s.clone(),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Value::Num(d) => format_number(*d),
            Value::Obj(o) => format!("{o:?}"),
            Value::Js(_) => "[object]".to_string(),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

fn parse_bool(s: &str) -> bool {
    let s = s.trim();
    !(s.is_empty() || s.eq_ignore_ascii_case("false") || s.eq_ignore_ascii_case("no") || s.eq_ignore_ascii_case("off") || s == "0")
}

/// Like JScript's loose number parsing: hex, a trailing "px", or a numeric prefix like parseFloat.
pub fn parse_number(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
        && let Ok(v) = i64::from_str_radix(hex, 16)
    {
        return v as f64;
    }
    let s = s.strip_suffix("px").or_else(|| s.strip_suffix("PX")).unwrap_or(s);
    if let Ok(d) = s.parse::<f64>()
        && (d.is_finite() || s.to_ascii_lowercase().contains("inf"))
    {
        return d;
    }
    if s.eq_ignore_ascii_case("true") {
        return 1.0;
    }
    let prefix: String =
        s.char_indices().take_while(|&(i, c)| c.is_ascii_digit() || c == '.' || (i == 0 && (c == '-' || c == '+'))).map(|(_, c)| c).collect();
    prefix.parse::<f64>().unwrap_or(0.0)
}

/// Integers print without a decimal point, as JScript did.
pub fn format_number(d: f64) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.abs() < 1e15 && d == d.floor() {
        return format!("{}", d as i64);
    }
    format!("{d}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerces_like_wmp() {
        assert!(!Value::from("false").to_bool());
        assert!(!Value::from(" off ").to_bool());
        assert!(Value::from("yes").to_bool());
        assert_eq!(Value::from("12px").to_number(), 12.0);
        assert_eq!(Value::from("0x1F").to_number(), 31.0);
        assert_eq!(Value::from("3.5abc").to_number(), 3.5);
        assert_eq!(Value::from("abc").to_number(), 0.0);
        assert_eq!(Value::from(7.9).coerce(PType::Int), Value::Num(7.0));
        assert_eq!(Value::Num(3.0).to_str(), "3");
        assert_eq!(Value::Num(0.25).to_str(), "0.25");
    }
}
