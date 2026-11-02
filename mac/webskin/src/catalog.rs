//! Who published each skin (the brand it was made for: Microsoft, Disney, Warner Bros...) and who designed
//! it (The Skins Factory, SkinWerkz...), for the Skin Chooser. Both come from the copyright and author a
//! skin carries, through the rules in the Windows app's SkinCatalog.json, whose "skins" list corrects the
//! skins whose own text says too little or misleads. The same file serves both apps.

use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const INDEPENDENT: &str = "Independent artists";
pub const OTHER: &str = "Other";

const CATALOG: &str = include_str!("../../../src/WebSkinPlayer/Skins/SkinCatalog.json");

struct Catalog {
    publishers: Vec<(Regex, String)>,
    designers: Vec<(Regex, String)>,
    known: HashMap<String, (String, Option<String>)>,
}

fn catalog() -> &'static Catalog {
    static CELL: OnceLock<Catalog> = OnceLock::new();
    CELL.get_or_init(|| {
        // The file has // comment lines, which JSON doesn't.
        let text: String = CATALOG.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let root: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let rules = |name: &str| -> Vec<(Regex, String)> {
            root.get(name)
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|r| {
                    let pattern = r.get(0)?.as_str()?;
                    let label = r.get(1)?.as_str()?;
                    Some((Regex::new(&format!("(?i){pattern}")).ok()?, label.to_string()))
                })
                .collect()
        };
        let known = root
            .get("skins")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flatten()
            .map(|(k, v)| {
                let publisher = v.get(0).and_then(serde_json::Value::as_str).unwrap_or(OTHER).to_string();
                let designer = v.get(1).and_then(serde_json::Value::as_str).map(str::to_string);
                (k.clone(), (publisher, designer))
            })
            .collect();
        Catalog { publishers: rules("publishers"), designers: rules("designers"), known }
    })
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid pattern")
}

/// "Kenwood_MPXP.wmz", "Plus! Mecha" and "Alienware_Invader_by_The_Skins_Factory" become "kenwood", "mecha"
/// and "alienwareinvader", so the different builds of one skin share an entry.
pub fn key(file_or_title: &str) -> String {
    static RULES: OnceLock<[Regex; 5]> = OnceLock::new();
    let [parens, plus, the, version, other] = RULES.get_or_init(|| {
        [re(r"\(.*?\)"), re(r"^plus!?_?\s*"), re(r"^the[ _]"), re(r"[ _-](mp7|mpxp|mp71|wmp1[01]|wmp[789]|v\d+(\.\d+)*)\s*$"), re("[^a-z0-9]")]
    });
    let stem = std::path::Path::new(file_or_title).file_stem().map_or_else(|| file_or_title.to_string(), |s| s.to_string_lossy().into_owned());
    let mut t = stem.to_lowercase();
    // A title has no extension; only cut one that looks like a skin's.
    if !(file_or_title.to_lowercase().ends_with(".wmz") || file_or_title.to_lowercase().ends_with(".wms")) {
        t = file_or_title.to_lowercase();
    }
    let t = parens.replace_all(&t, " ");
    let t = plus.replace(&t, "");
    let t = the.replace(&t, "");
    let t = t.replace("_by_the_skins_factory", "");
    let t = version.replace(&t, "");
    other.replace_all(&t, "").into_owned()
}

/// A file name as a readable name: "Windows_XP.wmz" becomes "Windows XP".
pub fn pretty_name(file: &str) -> String {
    let stem = std::path::Path::new(file).file_stem().map_or_else(|| file.to_string(), |s| s.to_string_lossy().into_owned());
    stem.replace('_', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The publisher and designer of a skin, from its file name and the author and copyright it carries.
pub fn lookup(file: &str, author: &str, copyright: &str) -> (String, Option<String>) {
    let c = catalog();
    let designer = matching(&c.designers, author).or_else(|| matching(&c.designers, copyright)).or_else(|| person(author));
    if let Some((publisher, known_designer)) = c.known.get(&key(file)) {
        return (publisher.clone(), known_designer.clone().or(designer));
    }
    if let Some(publisher) = matching(&c.publishers, copyright).or_else(|| matching(&c.publishers, author)) {
        return (publisher, designer);
    }
    match designer {
        Some(d) => (INDEPENDENT.to_string(), Some(d)),
        None => (OTHER.to_string(), None),
    }
}

fn matching(rules: &[(Regex, String)], text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    rules.iter().find(|(r, _)| r.is_match(text)).map(|(_, name)| name.clone())
}

/// An author line that names a person: "Designer: Joe Walker" becomes "Joe Walker"; placeholders and URLs drop out.
fn person(author: &str) -> Option<String> {
    static RULES: OnceLock<[Regex; 2]> = OnceLock::new();
    let [label, tail] = RULES.get_or_init(|| [re(r"(?i)^(designers?|design)\s*:\s*"), re(r"\s*[-/,(:].*$")]);
    let a = author.trim();
    if a.is_empty() || a.to_lowercase().starts_with("res:") || a.contains('@') {
        return None;
    }
    let a = label.replace(a, "");
    let a = tail.replace(&a, "");
    let a = a.trim();
    (!a.is_empty() && a.chars().count() < 60).then(|| a.to_string())
}

/// Sorts publishers by name, with independent artists and unknown skins last.
pub fn sort_key(publisher: &str) -> String {
    match publisher {
        INDEPENDENT => "\u{FFFE}".into(),
        OTHER => "\u{FFFF}".into(),
        p => p.to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_join_the_builds_of_one_skin() {
        assert_eq!(key("Kenwood_MPXP.wmz"), "kenwood");
        assert_eq!(key("Plus! Mecha.wmz"), "mecha");
        assert_eq!(key("Alienware_Invader_by_The_Skins_Factory.wmz"), "alienwareinvader");
        assert_eq!(key("The_Sentinel.wmz"), "sentinel");
    }

    #[test]
    fn publishers_come_from_the_copyright_then_the_known_list() {
        assert_eq!(lookup("Whatever.wmz", "", "(c) 2004 Disney Enterprises").0, "Disney");
        assert_eq!(lookup("Whatever.wmz", "The Skins Factory", "").1.as_deref(), Some("The Skins Factory"));
        assert_eq!(lookup("Darkling.wmz", "", "").0, "Microsoft");
        let (publisher, designer) = lookup("Mine.wmz", "Designer: Joe Walker - joe.example", "");
        assert_eq!((publisher.as_str(), designer.as_deref()), (INDEPENDENT, Some("Joe Walker")));
        assert_eq!(lookup("Mystery.wmz", "", "").0, OTHER);
    }

    #[test]
    fn names_read_well_and_sort_with_the_unknown_last() {
        assert_eq!(pretty_name("Windows_XP__Classic.wmz"), "Windows XP Classic");
        assert!(sort_key("Disney") < sort_key(INDEPENDENT));
        assert!(sort_key(INDEPENDENT) < sort_key(OTHER));
    }
}
