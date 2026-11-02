//! Looks for a newer GitHub release, as the Windows app does: once per start, against the repository the
//! build came from (local builds don't know one and never check). The release's Mac zip is what
//! Install Now downloads, checked against the checksum GitHub publishes for it.

use std::time::Duration;

/// A newer release.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateInfo {
    /// "0.4.0", without the v.
    pub version: String,
    /// The release page (What's New).
    pub page_url: String,
    /// The release's WebSkinPlayer-x.y.z-mac.zip, when it has one.
    pub zip_url: Option<String>,
    pub size: u64,
    /// The zip's SHA-256 as GitHub reports it (lower-case hex), if it does.
    pub sha256: Option<String>,
}

/// The releases/latest feed of a GitHub repository URL ("https://github.com/owner/repo").
pub fn feed_for(repository: &str) -> Option<String> {
    let rest = repository.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = rest.split_once("github.com/")?.1;
    let mut parts = path.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty() && parts.next().is_none()).then(|| format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"))
}

fn core(v: &str) -> Vec<u64> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let v = v.split(['-', '+']).next().unwrap_or_default();
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn is_prerelease(v: &str) -> bool {
    v.split('+').next().unwrap_or_default().contains('-')
}

/// A higher version, or the release of the beta (or CI build) running: 0.3.0 is newer than 0.3.0-beta.1.
pub fn is_newer(tag: &str, current: &str) -> bool {
    let (mut latest, mut mine) = (core(tag), core(current));
    let n = latest.len().max(mine.len()).max(3);
    latest.resize(n, 0);
    mine.resize(n, 0);
    latest > mine || (latest == mine && is_prerelease(current) && !is_prerelease(tag))
}

/// The update a releases/latest answer offers to a copy running `current`, if it's newer.
pub fn parse_release(json: &[u8], current: &str) -> Option<UpdateInfo> {
    let root: serde_json::Value = serde_json::from_slice(json).ok()?;
    let tag = root.get("tag_name")?.as_str()?;
    if !is_newer(tag, current) {
        return None;
    }
    let page_url = root.get("html_url").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    let asset = root
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|a| a.get("name").and_then(serde_json::Value::as_str).is_some_and(|n| n.to_lowercase().ends_with("-mac.zip")));
    let text = |key: &str| asset.and_then(|a| a.get(key)).and_then(serde_json::Value::as_str).map(str::to_string);
    Some(UpdateInfo {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        page_url,
        zip_url: text("browser_download_url"),
        size: asset.and_then(|a| a.get("size")).and_then(serde_json::Value::as_u64).unwrap_or(0),
        sha256: text("digest").and_then(|d| d.strip_prefix("sha256:").map(str::to_lowercase)),
    })
}

/// Asks the feed (blocking: call it off the main thread).
pub fn check(feed: &str, current: &str) -> Result<Option<UpdateInfo>, String> {
    let json = crate::net::get(feed, &["Accept: application/vnd.github+json"], Duration::from_secs(20))?;
    Ok(parse_release(&json, current))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_as_the_windows_app_does() {
        assert!(is_newer("v0.4.0", "0.3.0"));
        assert!(is_newer("v0.3.1", "0.3.0"));
        assert!(!is_newer("v0.3.0", "0.3.0"));
        assert!(!is_newer("v0.2.6", "0.3.0"));
        assert!(is_newer("v0.3.0", "0.3.0-beta.1"));
        assert!(is_newer("v0.3.0", "0.3.0-ci.42"));
        assert!(!is_newer("v0.3.0-beta.2", "0.3.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
    }

    #[test]
    fn finds_the_mac_zip_and_its_checksum() {
        let json = br#"{"tag_name":"v0.4.0","html_url":"https://github.com/o/r/releases/tag/v0.4.0","assets":[
            {"name":"WebSkinPlayer-0.4.0-Setup.exe","browser_download_url":"https://x/setup.exe","size":5},
            {"name":"WebSkinPlayer-0.4.0-mac.zip","browser_download_url":"https://x/mac.zip","size":3909954,
             "digest":"sha256:9ABB0E937E7C2C3D169348AD46756B507CAFF546814B8D04D63332C70C8168CA"}]}"#;
        let u = parse_release(json, "0.3.0").unwrap();
        assert_eq!(u.version, "0.4.0");
        assert_eq!(u.zip_url.as_deref(), Some("https://x/mac.zip"));
        assert_eq!(u.size, 3_909_954);
        assert_eq!(u.sha256.as_deref(), Some("9abb0e937e7c2c3d169348ad46756b507caff546814b8d04d63332c70c8168ca"));
        assert!(parse_release(json, "0.4.0").is_none());
    }

    #[test]
    fn reads_the_feed_from_the_repository_url() {
        assert_eq!(
            feed_for("https://github.com/bugshotss/WMP-Web-Player").as_deref(),
            Some("https://api.github.com/repos/bugshotss/WMP-Web-Player/releases/latest")
        );
        assert_eq!(
            feed_for("https://github.com/bugshotss/WMP-Web-Player.git/").as_deref(),
            feed_for("https://github.com/bugshotss/WMP-Web-Player").as_deref()
        );
        assert!(feed_for("").is_none());
        assert!(feed_for("https://example.com/x").is_none());
    }
}
