//! Daily check for a newer stable GitHub Release.
//!
//! Matches the installer's notion of "latest": the highest stable `vX.Y.Z`
//! tag, not GitHub's "Latest" badge. The result is cached in the store dir so
//! the GUI hits the network at most once per [`CHECK_TTL_MS`], and a network
//! failure falls back to the cached answer instead of surfacing an error —
//! an update badge is never worth an error banner.

use crate::paths::store_dir;
use crate::secure_fs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

const RELEASES_URL: &str =
    "https://api.github.com/repos/Lumisia/aho__ai-handoff/releases?per_page=30";
const CHECK_TTL_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateStatus {
    pub current: String,
    pub latest: Option<String>,
    pub update_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckCache {
    checked_at_ms: i64,
    /// Highest stable tag seen at `checked_at_ms` (e.g. `"v2.3.0"`), or `None`
    /// when the last check could not resolve one.
    latest_tag: Option<String>,
}

fn cache_path() -> PathBuf {
    store_dir().join("update-check.json")
}

/// Resolve the newest stable release, consulting the daily cache first.
/// Never fails: any error degrades to `latest: None` (no badge).
pub fn check(current_version: &str) -> UpdateStatus {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let cached = read_cache();
    let latest_tag = match &cached {
        Some(cache) if now_ms - cache.checked_at_ms < CHECK_TTL_MS => cache.latest_tag.clone(),
        _ => {
            let fetched = fetch_latest_stable_tag();
            match fetched {
                Ok(tag) => {
                    write_cache(&CheckCache {
                        checked_at_ms: now_ms,
                        latest_tag: tag.clone(),
                    });
                    tag
                }
                // Network failure: reuse the stale answer (if any) and leave
                // the cache untouched so the next launch retries.
                Err(()) => cached.and_then(|cache| cache.latest_tag),
            }
        }
    };
    status_from(current_version, latest_tag)
}

/// Pure decision: given the current version and the newest stable tag, is an
/// update available?
fn status_from(current_version: &str, latest_tag: Option<String>) -> UpdateStatus {
    let update_available = match (
        parse_stable_version(current_version),
        latest_tag.as_deref().and_then(parse_stable_version),
    ) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    };
    UpdateStatus {
        current: current_version.to_string(),
        latest: latest_tag,
        update_available,
    }
}

/// Parse `X.Y.Z` (with or without a leading `v`) into a comparable tuple.
/// Anything with a suffix (`-rc1`, `+build`) is not stable and yields `None`.
fn parse_stable_version(tag: &str) -> Option<(u64, u64, u64)> {
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    let mut parts = tag.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// The highest stable tag among release entries (draft/prerelease excluded).
fn pick_latest_stable(releases: &serde_json::Value) -> Option<String> {
    releases
        .as_array()?
        .iter()
        .filter(|release| {
            !release["draft"].as_bool().unwrap_or(false)
                && !release["prerelease"].as_bool().unwrap_or(false)
        })
        .filter_map(|release| release["tag_name"].as_str())
        .filter_map(|tag| parse_stable_version(tag).map(|version| (version, tag)))
        .max_by_key(|(version, _)| *version)
        .map(|(_, tag)| tag.to_string())
}

fn fetch_latest_stable_tag() -> Result<Option<String>, ()> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(6))
        .user_agent("ai-handoff")
        .build();
    let body: serde_json::Value = agent
        .get(RELEASES_URL)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|_| ())?
        .into_json()
        .map_err(|_| ())?;
    Ok(pick_latest_stable(&body))
}

fn read_cache() -> Option<CheckCache> {
    let bytes = std::fs::read(cache_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_cache(cache: &CheckCache) {
    let _ = secure_fs::ensure_private_dir(&store_dir());
    if let Ok(json) = serde_json::to_vec_pretty(cache) {
        let path = cache_path();
        let tmp = path.with_extension("json.tmp");
        let _ = secure_fs::write_private_atomic(&path, &tmp, &json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stable_versions_parse_and_prereleases_do_not() {
        assert_eq!(parse_stable_version("v2.3.0"), Some((2, 3, 0)));
        assert_eq!(parse_stable_version("2.3.0"), Some((2, 3, 0)));
        assert_eq!(parse_stable_version("v10.0.12"), Some((10, 0, 12)));
        assert_eq!(parse_stable_version("v2.3.0-rc1"), None);
        assert_eq!(parse_stable_version("v2.3"), None);
        assert_eq!(parse_stable_version("v2.3.0.1"), None);
        assert_eq!(parse_stable_version("nightly"), None);
    }

    #[test]
    fn picks_highest_stable_tag_ignoring_drafts_prereleases_and_badge_order() {
        // GitHub's "Latest" badge order is the array order; the highest stable
        // tag wins regardless of position.
        let releases = json!([
            { "tag_name": "v2.2.9", "draft": false, "prerelease": false },
            { "tag_name": "v2.10.0", "draft": false, "prerelease": false },
            { "tag_name": "v3.0.0-rc1", "draft": false, "prerelease": false },
            { "tag_name": "v9.9.9", "draft": true, "prerelease": false },
            { "tag_name": "v4.0.0", "draft": false, "prerelease": true },
        ]);
        assert_eq!(pick_latest_stable(&releases).as_deref(), Some("v2.10.0"));
        assert_eq!(pick_latest_stable(&json!([])), None);
    }

    #[test]
    fn update_available_only_when_latest_is_strictly_newer() {
        assert!(status_from("2.2.3", Some("v2.3.0".into())).update_available);
        assert!(!status_from("2.3.0", Some("v2.3.0".into())).update_available);
        assert!(!status_from("2.4.0", Some("v2.3.0".into())).update_available);
        assert!(!status_from("2.2.3", None).update_available);
        // A dev build with a suffix never claims an update.
        assert!(!status_from("2.3.0-dev", Some("v9.9.9".into())).update_available);
    }
}
