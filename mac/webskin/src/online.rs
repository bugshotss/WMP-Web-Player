//! The "Windows Media Player Skins Collection" on archive.org: 175 .wmz files, each with a screenshot
//! taken in WMP. The app ships without skins and downloads them from here on request.

use std::time::Duration;

pub const ITEM: &str = "windowsmediaplayerskinscollection";
pub const PAGE_URL: &str = "https://archive.org/details/windowsmediaplayerskinscollection";

/// A skin in the collection.
#[derive(Clone, Debug, PartialEq)]
pub struct OnlineSkin {
    pub file: String,
    pub size: u64,
}

impl OnlineSkin {
    pub fn url(&self) -> String {
        file_url(&self.file)
    }

    /// The screenshot of the skin taken in WMP, published with it.
    pub fn preview_url(&self) -> String {
        let stem = self.file.rsplit_once('.').map_or(self.file.as_str(), |(s, _)| s);
        file_url(&format!("{stem}.png"))
    }
}

pub fn file_url(name: &str) -> String {
    format!("https://archive.org/download/{ITEM}/{}", escape(name))
}

/// Percent-encodes everything but RFC 3986's unreserved characters.
fn escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The skins in archive.org's file listing (`/metadata/<item>/files`), by name.
pub fn parse_listing(json: &[u8]) -> Vec<OnlineSkin> {
    let Ok(root) = serde_json::from_slice::<serde_json::Value>(json) else { return vec![] };
    let mut list: Vec<OnlineSkin> = root
        .get("result")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let name = f.get("name")?.as_str()?;
            if !name.to_lowercase().ends_with(".wmz") || name.contains('/') {
                return None;
            }
            let size = match f.get("size") {
                Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0),
                Some(v) => v.as_u64().unwrap_or(0),
                None => 0,
            };
            Some(OnlineSkin { file: name.to_string(), size })
        })
        .collect();
    list.sort_by_key(|s| crate::catalog::pretty_name(&s.file).to_lowercase());
    list
}

/// Fetches the collection's listing (blocking: call it off the main thread).
pub fn list() -> Result<Vec<OnlineSkin>, String> {
    let json = crate::net::get(&format!("https://archive.org/metadata/{ITEM}/files"), &[], Duration::from_secs(30))?;
    let list = parse_listing(&json);
    if list.is_empty() { Err("archive.org listed no skins".into()) } else { Ok(list) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_wmz_files_with_their_sizes() {
        let json = br#"{"result":[
            {"name":"Windows_XP.wmz","size":"123456"},
            {"name":"Windows_XP.png","size":"999"},
            {"name":"sub/Other.wmz","size":"1"},
            {"name":"Alienware Invader.wmz","size":2048}
        ]}"#;
        let list = parse_listing(json);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], OnlineSkin { file: "Alienware Invader.wmz".into(), size: 2048 });
        assert_eq!(list[1].size, 123_456);
        assert_eq!(list[0].url(), "https://archive.org/download/windowsmediaplayerskinscollection/Alienware%20Invader.wmz");
        assert!(list[1].preview_url().ends_with("/Windows_XP.png"));
    }
}
