use serde_json::Value;
use std::path::{Path, PathBuf};

/// Rate-limit sample used by the five-hour trigger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TriggerUsage {
    pub used_percent: f64,
    /// Unix epoch milliseconds for the active window reset, when known.
    pub resets_at_ms: Option<i64>,
}

pub fn used_percent_from_jsonl(path: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(parse_used_percent_line)
}

fn parse_used_percent_line(line: &str) -> Option<f64> {
    let value: Value = serde_json::from_str(line).ok()?;
    value
        .get("payload")?
        .get("rate_limits")?
        .get("primary")?
        .get("used_percent")?
        .as_f64()
}

pub fn codex_trigger_usage_from_jsonl(path: &Path, now_ms: i64) -> Option<TriggerUsage> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(|line| parse_codex_trigger_usage_line(line, now_ms))
}

fn parse_codex_trigger_usage_line(line: &str, now_ms: i64) -> Option<TriggerUsage> {
    let value: Value = serde_json::from_str(line).ok()?;
    trigger_usage_from_rate_limits(value.get("payload")?.get("rate_limits")?, now_ms)
}

/// Parse a `rate_limits` object (`primary.used_percent` + reset hints) into a
/// [`TriggerUsage`]. Shared by the rollout-line parser and the raw hook input.
fn trigger_usage_from_rate_limits(rate_limits: &Value, now_ms: i64) -> Option<TriggerUsage> {
    let primary = rate_limits.get("primary")?;
    let used_percent = primary.get("used_percent")?.as_f64()?;
    if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
        return None;
    }
    Some(TriggerUsage {
        used_percent,
        resets_at_ms: reset_at_ms(primary, now_ms),
    })
}

/// `rate_limits` carried directly by a Codex hook input, either under
/// `payload.rate_limits` (rollout record shape) or top-level `rate_limits`.
pub fn codex_trigger_usage_from_raw(raw: &Value, now_ms: i64) -> Option<TriggerUsage> {
    let rate_limits = raw
        .get("payload")
        .and_then(|payload| payload.get("rate_limits"))
        .or_else(|| raw.get("rate_limits"))?;
    trigger_usage_from_rate_limits(rate_limits, now_ms)
}

/// `rate_limits` carried directly by a Claude hook/statusline-shaped payload.
/// Claude names the 5-hour window `five_hour` and the usage field
/// `used_percentage`, so it cannot share the Codex `primary.used_percent`
/// parser.
pub fn claude_trigger_usage_from_raw(raw: &Value, now_ms: i64) -> Option<TriggerUsage> {
    let rate_limits = raw
        .get("payload")
        .and_then(|payload| payload.get("rate_limits"))
        .or_else(|| raw.get("rate_limits"))?;
    let window = rate_limits.get("five_hour")?;
    let used_percent = window
        .get("used_percentage")
        .or_else(|| window.get("used_percent"))?
        .as_f64()?;
    if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
        return None;
    }
    Some(TriggerUsage {
        used_percent,
        resets_at_ms: reset_at_ms(window, now_ms),
    })
}

/// The Codex rollout roots: `<CODEX_HOME>/sessions` and
/// `<CODEX_HOME>/archived_sessions`.
pub fn codex_sessions_dirs() -> Vec<PathBuf> {
    crate::account::codex_home()
        .map(|home| vec![home.join("sessions"), home.join("archived_sessions")])
        .unwrap_or_default()
}

/// Find the newest `rollout-*.jsonl` whose file name contains `session_id`
/// under the given roots. Only directory entries are inspected — no file
/// contents are read — so the walk stays cheap even on large session trees.
pub fn find_codex_rollout(roots: &[PathBuf], session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() {
        return None;
    }
    find_best_codex_rollout(roots, |name| name.contains(session_id))
}

/// Find the newest `rollout-*.jsonl` under the given roots, regardless of
/// session id. This is a best-effort fallback for hook payloads that do not
/// expose the session id shape used in the rollout file name.
pub fn find_latest_codex_rollout(roots: &[PathBuf]) -> Option<PathBuf> {
    find_best_codex_rollout(roots, |_| true)
}

fn find_best_codex_rollout<F>(roots: &[PathBuf], mut matches_name: F) -> Option<PathBuf>
where
    F: FnMut(&str) -> bool,
{
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    let mut stack: Vec<PathBuf> = roots.to_vec();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") || !matches_name(name) {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(_, current)| modified > *current) {
                best = Some((path, modified));
            }
        }
    }
    best.map(|(path, _)| path)
}

/// Where a Codex usage sample came from, or why every source yielded nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct CodexUsageResolution {
    pub usage: Option<TriggerUsage>,
    /// `"raw-rate-limits"`, `"transcript-path"`, `"session-rollout"`, or
    /// `"latest-rollout"`.
    pub source: Option<&'static str>,
    /// The rollout file that produced (or should have produced) the sample —
    /// cacheable by the caller to skip the directory walk next time.
    pub rollout_path: Option<PathBuf>,
    /// Why each source failed, in resolution order, when `usage` is `None`.
    pub unknown_reasons: Vec<&'static str>,
}

/// Resolve the Codex five-hour usage for a hook event. The hook input is not
/// guaranteed to carry `transcript_path`, so this tries, in order:
/// 1. `rate_limits` embedded in the raw hook input (freshest, no I/O),
/// 2. the transcript JSONL the hook points at,
/// 3. the session's own rollout file located by `session_id` under
///    `sessions_dirs` (`cached_rollout` skips the walk when still valid),
/// 4. the newest rollout file under `sessions_dirs` as a last local fallback.
pub fn resolve_codex_trigger_usage(
    raw: &Value,
    transcript_path: Option<&Path>,
    session_id: Option<&str>,
    sessions_dirs: &[PathBuf],
    cached_rollout: Option<&Path>,
    now_ms: i64,
) -> CodexUsageResolution {
    let mut reasons = Vec::new();

    match codex_trigger_usage_from_raw(raw, now_ms) {
        Some(usage) => {
            return CodexUsageResolution {
                usage: Some(usage),
                source: Some("raw-rate-limits"),
                rollout_path: None,
                unknown_reasons: Vec::new(),
            }
        }
        None => reasons.push("no-raw-rate-limits"),
    }

    match transcript_path {
        Some(path) => match codex_trigger_usage_from_jsonl(path, now_ms) {
            Some(usage) => {
                return CodexUsageResolution {
                    usage: Some(usage),
                    source: Some("transcript-path"),
                    rollout_path: Some(path.to_path_buf()),
                    unknown_reasons: Vec::new(),
                }
            }
            None => reasons.push("transcript-no-rate-limits"),
        },
        None => reasons.push("no-transcript-path"),
    }

    let Some(session_id) = session_id.filter(|sid| !sid.is_empty()) else {
        reasons.push("no-session-id");
        return latest_codex_rollout_resolution(sessions_dirs, now_ms, reasons);
    };
    let rollout = cached_rollout
        .filter(|path| path.is_file())
        .map(Path::to_path_buf)
        .or_else(|| find_codex_rollout(sessions_dirs, session_id));
    match rollout {
        Some(path) => match codex_trigger_usage_from_jsonl(&path, now_ms) {
            Some(usage) => CodexUsageResolution {
                usage: Some(usage),
                source: Some("session-rollout"),
                rollout_path: Some(path),
                unknown_reasons: Vec::new(),
            },
            None => {
                reasons.push("rollout-no-rate-limits");
                latest_codex_rollout_resolution(sessions_dirs, now_ms, reasons)
            }
        },
        None => {
            reasons.push("session-rollout-not-found");
            latest_codex_rollout_resolution(sessions_dirs, now_ms, reasons)
        }
    }
}

fn latest_codex_rollout_resolution(
    sessions_dirs: &[PathBuf],
    now_ms: i64,
    mut reasons: Vec<&'static str>,
) -> CodexUsageResolution {
    match find_latest_codex_rollout(sessions_dirs) {
        Some(path) => match codex_trigger_usage_from_jsonl(&path, now_ms) {
            Some(usage) => CodexUsageResolution {
                usage: Some(usage),
                source: Some("latest-rollout"),
                rollout_path: Some(path),
                unknown_reasons: Vec::new(),
            },
            None => {
                reasons.push("latest-rollout-no-rate-limits");
                CodexUsageResolution {
                    usage: None,
                    source: None,
                    rollout_path: Some(path),
                    unknown_reasons: reasons,
                }
            }
        },
        None => {
            reasons.push("latest-rollout-not-found");
            CodexUsageResolution {
                usage: None,
                source: None,
                rollout_path: None,
                unknown_reasons: reasons,
            }
        }
    }
}

fn reset_at_ms(window: &Value, now_ms: i64) -> Option<i64> {
    for key in [
        "resets_at_ms",
        "reset_at_ms",
        "resets_at_millis",
        "reset_at_millis",
    ] {
        if let Some(ms) = window.get(key).and_then(numeric_value) {
            return future_epoch_ms(ms.round() as i64, now_ms);
        }
    }

    for key in ["resets_at", "reset_at"] {
        if let Some(value) = window.get(key) {
            if let Some(ms) = absolute_epoch_ms(value) {
                return future_epoch_ms(ms, now_ms);
            }
        }
    }

    for key in ["resets_in_ms", "reset_in_ms", "reset_after_ms"] {
        if let Some(ms) = window.get(key).and_then(numeric_value) {
            let delta = ms.max(0.0).round() as i64;
            return Some(now_ms.saturating_add(delta));
        }
    }

    for key in [
        "resets_in_seconds",
        "reset_in_seconds",
        "reset_after_seconds",
        "retry_after_seconds",
    ] {
        if let Some(seconds) = window.get(key).and_then(numeric_value) {
            let delta = (seconds.max(0.0) * 1000.0).round() as i64;
            return Some(now_ms.saturating_add(delta));
        }
    }

    None
}

fn numeric_value(value: &Value) -> Option<f64> {
    let number = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))?;
    number.is_finite().then_some(number)
}

fn absolute_epoch_ms(value: &Value) -> Option<i64> {
    if let Some(number) = numeric_value(value) {
        if number <= 0.0 {
            return None;
        }
        return Some(if number >= 10_000_000_000.0 {
            number.round() as i64
        } else {
            (number * 1000.0).round() as i64
        });
    }
    let text = value.as_str()?;
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn future_epoch_ms(ms: i64, now_ms: i64) -> Option<i64> {
    (ms > now_ms).then_some(ms)
}

// ---------------------------------------------------------------------------
// Claude rate-limit sensor
// ---------------------------------------------------------------------------

/// A Claude rate-limit sample captured from the statusline payload.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaudeWindow {
    pub used_percent: f64,
    pub window_minutes: u32,
    pub resets_at: Option<f64>,
}

/// A Claude rate-limit sample captured from the statusline payload.
pub struct ClaudeUsage {
    pub used_percent: f64,
    pub window_minutes: u32,
    pub resets_at: Option<f64>,
    pub weekly: Option<ClaudeWindow>,
    pub source: &'static str,
    pub captured_at: i64,
}

/// Compute the lowercase-hex SHA-256 of the given bytes (full 64 hex chars).
fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let digest = h.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest.iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

fn statusline_window(
    input: &serde_json::Value,
    name: &str,
    window_minutes: u32,
) -> Option<ClaudeWindow> {
    let window = input.get("rate_limits").and_then(|rl| rl.get(name))?;
    let used_percent = window.get("used_percentage").and_then(|v| v.as_f64())?;
    if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
        return None;
    }
    let resets_at = window.get("resets_at").and_then(|v| v.as_f64());
    Some(ClaudeWindow {
        used_percent,
        window_minutes,
        resets_at,
    })
}

/// Record a Claude rate-limit sample from a statusline JSON payload.
///
/// Extracts `rate_limits.five_hour`, optional `rate_limits.seven_day`,
/// and `session_id` from `input`. Writes a sample JSON file under
/// `paths::rate_limits_dir()` atomically (tmp + rename). Returns `true`
/// iff the sample was written successfully.
///
/// Returns `false` (and writes nothing, never panics) when:
/// - `session_id` is absent or empty
/// - `used_percentage` is absent, non-finite, or outside [0.0, 100.0]
pub fn record_claude_rate_limit(input: &serde_json::Value, now_ms: i64) -> bool {
    // Extract and validate session_id.
    let session_id = match input.get("session_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };

    let five_hour = match statusline_window(input, "five_hour", 300) {
        Some(window) => window,
        None => return false,
    };
    let weekly = statusline_window(input, "seven_day", 10080);

    // Ensure the directory exists.
    let dir = crate::paths::rate_limits_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }

    // Build the target file path.
    let file_name = format!("{}.json", sha256_hex(session_id));
    let target = dir.join(&file_name);
    let tmp = dir.join(format!("{}.tmp", file_name));

    // Build the sample JSON.
    let sample = serde_json::json!({
        "session_id": session_id,
        "used_percent": five_hour.used_percent,
        "resets_at": five_hour.resets_at,
        "weekly": weekly.as_ref().map(|w| serde_json::json!({
            "used_percent": w.used_percent,
            "window_minutes": w.window_minutes,
            "resets_at": w.resets_at,
        })),
        "captured_at": now_ms,
    });
    let json = match serde_json::to_vec(&sample) {
        Ok(b) => b,
        Err(_) => return false,
    };

    // Atomic write: write to tmp then rename over target.
    if std::fs::write(&tmp, &json).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    match std::fs::rename(&tmp, &target) {
        Ok(()) => true,
        Err(_) => {
            let _ = std::fs::remove_file(&tmp);
            false
        }
    }
}

/// How old a sample without a `resets_at` may be and still drive the trigger.
const TRIGGER_STRICT_FRESHNESS_MS: i64 = 10 * 60 * 1000;

/// The Claude `used_percent` the five-hour trigger may act on, or `None` when
/// nothing trustworthy is recorded.
///
/// Within one five-hour window `used_percent` only ever grows, so any sample
/// whose `resets_at` is still in the future is a valid LOWER BOUND on the
/// current usage — a stale-but-unexpired sample that already crossed the
/// threshold means the live value has too. This keeps the trigger working
/// without any background polling or network fetches (deliberate: the daemon
/// must never burn CPU or call home while idle). Samples with no `resets_at`
/// carry no window proof, so they only count when captured recently.
pub fn claude_trigger_usage(now_ms: i64) -> Option<TriggerUsage> {
    // 24h scan window; read_claude_rate_limit already rejects expired resets_at.
    let usage = read_claude_rate_limit(24 * 60 * 60 * 1000, now_ms)?;
    match usage.resets_at {
        Some(resets_at) => Some(TriggerUsage {
            used_percent: usage.used_percent,
            resets_at_ms: Some((resets_at * 1000.0) as i64),
        }),
        None => {
            (now_ms - usage.captured_at <= TRIGGER_STRICT_FRESHNESS_MS).then_some(TriggerUsage {
                used_percent: usage.used_percent,
                resets_at_ms: None,
            })
        }
    }
}

pub fn claude_used_percent_for_trigger(now_ms: i64) -> Option<f64> {
    claude_trigger_usage(now_ms).map(|usage| usage.used_percent)
}

/// Scan all `*.json` files in `paths::rate_limits_dir()` and return the
/// freshest valid `ClaudeUsage` sample.
///
/// A sample is valid when:
/// - `now_ms - captured_at <= freshness_ms`
/// - `resets_at` is `None` OR `now_ms < (resets_at * 1000.0) as i64`
///
/// Unreadable or non-JSON files are silently skipped. Returns `None` when
/// the directory is missing or no valid samples exist.
pub fn read_claude_rate_limit(freshness_ms: i64, now_ms: i64) -> Option<ClaudeUsage> {
    let dir = crate::paths::rate_limits_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return None,
    };

    let mut best: Option<ClaudeUsage> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        // Parse the file; skip on any error.
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let v: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let used_percent = match v.get("used_percent").and_then(|x| x.as_f64()) {
            Some(p) => p,
            None => continue,
        };
        let captured_at = match v.get("captured_at").and_then(|x| x.as_i64()) {
            Some(t) => t,
            None => continue,
        };
        let resets_at: Option<f64> = v.get("resets_at").and_then(|x| x.as_f64());
        let weekly = v.get("weekly").and_then(|weekly| {
            let used_percent = weekly.get("used_percent").and_then(|x| x.as_f64())?;
            if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
                return None;
            }
            let window_minutes = weekly
                .get("window_minutes")
                .and_then(|x| x.as_u64())
                .and_then(|x| u32::try_from(x).ok())
                .unwrap_or(10080);
            let resets_at = weekly.get("resets_at").and_then(|x| x.as_f64());
            if let Some(ra) = resets_at {
                if now_ms >= (ra * 1000.0) as i64 {
                    return None;
                }
            }
            Some(ClaudeWindow {
                used_percent,
                window_minutes,
                resets_at,
            })
        });

        // Validate freshness.
        if now_ms - captured_at > freshness_ms {
            continue;
        }
        // Validate resets_at: skip if already past.
        if let Some(ra) = resets_at {
            if now_ms >= (ra * 1000.0) as i64 {
                continue;
            }
        }

        // Keep the freshest (largest captured_at).
        let is_better = best
            .as_ref()
            .map(|b| captured_at > b.captured_at)
            .unwrap_or(true);
        if is_better {
            best = Some(ClaudeUsage {
                used_percent,
                window_minutes: 300,
                resets_at,
                weekly,
                source: "claude-statusline",
                captured_at,
            });
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Codex JSONL tests (unchanged)
    // -----------------------------------------------------------------------

    #[test]
    fn jsonl_missing_file_is_none() {
        assert!(used_percent_from_jsonl(std::path::Path::new("/no/such.jsonl")).is_none());
    }

    #[test]
    fn jsonl_reads_latest_used_percent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.jsonl");
        std::fs::write(
            &p,
            "{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":12.5}}}}\n\
             {\"type\":\"x\"}\n\
             {\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":42.5}}}}\n",
        )
        .unwrap();
        assert_eq!(used_percent_from_jsonl(&p), Some(42.5));
    }

    #[test]
    fn jsonl_ignores_non_primary_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.jsonl");
        std::fs::write(&p, "{\"used_percent\":42.5}\n").unwrap();
        assert!(used_percent_from_jsonl(&p).is_none());
    }

    #[test]
    fn jsonl_trigger_usage_reads_primary_reset_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.jsonl");
        std::fs::write(
            &p,
            "{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":60.0,\"resets_at\":1750001800}}}}\n",
        )
        .unwrap();
        let sample = codex_trigger_usage_from_jsonl(&p, 1_750_000_000_000).unwrap();
        assert_eq!(sample.used_percent, 60.0);
        assert_eq!(sample.resets_at_ms, Some(1_750_001_800_000));
    }

    #[test]
    fn jsonl_trigger_usage_falls_back_when_reset_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.jsonl");
        std::fs::write(
            &p,
            "{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":61.0}}}}\n",
        )
        .unwrap();
        let sample = codex_trigger_usage_from_jsonl(&p, 1_750_000_000_000).unwrap();
        assert_eq!(sample.used_percent, 61.0);
        assert_eq!(sample.resets_at_ms, None);
    }

    // -----------------------------------------------------------------------
    // Codex usage resolver tests
    // -----------------------------------------------------------------------

    fn rate_limits_line(used_percent: f64) -> String {
        format!(
            "{{\"payload\":{{\"rate_limits\":{{\"primary\":{{\"used_percent\":{used_percent}}}}}}}}}\n"
        )
    }

    #[test]
    fn raw_rate_limits_parsed_from_payload_and_top_level() {
        let now_ms = 1_750_000_000_000;
        let nested = serde_json::json!({
            "payload": { "rate_limits": { "primary": { "used_percent": 29.0 } } }
        });
        assert_eq!(
            codex_trigger_usage_from_raw(&nested, now_ms)
                .unwrap()
                .used_percent,
            29.0
        );
        let top_level = serde_json::json!({
            "rate_limits": { "primary": { "used_percent": 12.0 } }
        });
        assert_eq!(
            codex_trigger_usage_from_raw(&top_level, now_ms)
                .unwrap()
                .used_percent,
            12.0
        );
        assert!(codex_trigger_usage_from_raw(&serde_json::json!({}), now_ms).is_none());
    }

    #[test]
    fn find_codex_rollout_picks_newest_match_across_roots() {
        let sessions = tempfile::tempdir().unwrap();
        let archived = tempfile::tempdir().unwrap();
        let day_dir = sessions.path().join("2026").join("07").join("05");
        std::fs::create_dir_all(&day_dir).unwrap();
        let sid = "0197e5c3-aaaa-bbbb-cccc-1234567890ab";

        let old = archived
            .path()
            .join(format!("rollout-2026-07-01T01-00-00-{sid}.jsonl"));
        std::fs::write(&old, rate_limits_line(10.0)).unwrap();
        let newer = day_dir.join(format!("rollout-2026-07-05T03-00-00-{sid}.jsonl"));
        std::fs::write(&newer, rate_limits_line(29.0)).unwrap();
        // Deterministic mtimes (filesystem timestamps can tie within a test).
        let times = |secs: u64| filetime::FileTime::from_unix_time(1_750_000_000 + secs as i64, 0);
        filetime::set_file_mtime(&old, times(0)).unwrap();
        filetime::set_file_mtime(&newer, times(60)).unwrap();
        // Unrelated files never match.
        std::fs::write(day_dir.join("rollout-other.jsonl"), rate_limits_line(99.0)).unwrap();

        let roots = vec![sessions.path().to_path_buf(), archived.path().to_path_buf()];
        assert_eq!(find_codex_rollout(&roots, sid), Some(newer));
        assert_eq!(find_codex_rollout(&roots, "no-such-session"), None);
        assert_eq!(find_codex_rollout(&roots, ""), None);
    }

    #[test]
    fn resolver_prefers_raw_then_transcript_then_session_rollout() {
        let now_ms = 1_750_000_000_000;
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("transcript.jsonl");
        std::fs::write(&transcript, rate_limits_line(40.0)).unwrap();

        // Raw wins over transcript.
        let raw = serde_json::json!({
            "payload": { "rate_limits": { "primary": { "used_percent": 29.0 } } }
        });
        let r = resolve_codex_trigger_usage(&raw, Some(&transcript), None, &[], None, now_ms);
        assert_eq!(r.usage.unwrap().used_percent, 29.0);
        assert_eq!(r.source, Some("raw-rate-limits"));

        // Transcript wins when raw has nothing.
        let r = resolve_codex_trigger_usage(
            &serde_json::json!({}),
            Some(&transcript),
            None,
            &[],
            None,
            now_ms,
        );
        assert_eq!(r.usage.unwrap().used_percent, 40.0);
        assert_eq!(r.source, Some("transcript-path"));
        assert_eq!(r.rollout_path.as_deref(), Some(transcript.as_path()));
    }

    #[test]
    fn resolver_falls_back_to_session_rollout_without_transcript_path() {
        let now_ms = 1_750_000_000_000;
        let sessions = tempfile::tempdir().unwrap();
        let day_dir = sessions.path().join("2026").join("07").join("05");
        std::fs::create_dir_all(&day_dir).unwrap();
        let sid = "0197e5c3-dddd-eeee-ffff-1234567890ab";
        let rollout = day_dir.join(format!("rollout-2026-07-05T03-00-00-{sid}.jsonl"));
        std::fs::write(&rollout, rate_limits_line(29.0)).unwrap();

        let roots = vec![sessions.path().to_path_buf()];
        let r = resolve_codex_trigger_usage(
            &serde_json::json!({}),
            None,
            Some(sid),
            &roots,
            None,
            now_ms,
        );
        assert_eq!(r.usage.unwrap().used_percent, 29.0);
        assert_eq!(r.source, Some("session-rollout"));
        assert_eq!(r.rollout_path.as_deref(), Some(rollout.as_path()));

        // A valid cached path skips the walk entirely (empty roots).
        let r = resolve_codex_trigger_usage(
            &serde_json::json!({}),
            None,
            Some(sid),
            &[],
            Some(&rollout),
            now_ms,
        );
        assert_eq!(r.usage.unwrap().used_percent, 29.0);
        assert_eq!(r.source, Some("session-rollout"));
    }

    #[test]
    fn resolver_reports_unknown_reasons_per_missing_source() {
        let now_ms = 1_750_000_000_000;
        let r = resolve_codex_trigger_usage(&serde_json::json!({}), None, None, &[], None, now_ms);
        assert!(r.usage.is_none());
        assert_eq!(
            r.unknown_reasons,
            vec![
                "no-raw-rate-limits",
                "no-transcript-path",
                "no-session-id",
                "latest-rollout-not-found"
            ]
        );

        let sessions = tempfile::tempdir().unwrap();
        let r = resolve_codex_trigger_usage(
            &serde_json::json!({}),
            None,
            Some("sid-missing"),
            &[sessions.path().to_path_buf()],
            None,
            now_ms,
        );
        assert_eq!(
            r.unknown_reasons,
            vec![
                "no-raw-rate-limits",
                "no-transcript-path",
                "session-rollout-not-found",
                "latest-rollout-not-found"
            ]
        );
    }

    #[test]
    fn resolver_falls_back_to_latest_rollout_when_hook_lacks_session_match() {
        let now_ms = 1_750_000_000_000;
        let sessions = tempfile::tempdir().unwrap();
        let day_dir = sessions.path().join("2026").join("07").join("05");
        std::fs::create_dir_all(&day_dir).unwrap();
        let old = day_dir.join("rollout-2026-07-05T01-00-00-old-session.jsonl");
        let latest = day_dir.join("rollout-2026-07-05T03-00-00-current-session.jsonl");
        std::fs::write(&old, rate_limits_line(12.0)).unwrap();
        std::fs::write(&latest, rate_limits_line(78.0)).unwrap();
        let times = |secs: u64| filetime::FileTime::from_unix_time(1_750_000_000 + secs as i64, 0);
        filetime::set_file_mtime(&old, times(0)).unwrap();
        filetime::set_file_mtime(&latest, times(60)).unwrap();

        let r = resolve_codex_trigger_usage(
            &serde_json::json!({}),
            None,
            Some("session-id-not-in-filename"),
            &[sessions.path().to_path_buf()],
            None,
            now_ms,
        );
        assert_eq!(r.usage.unwrap().used_percent, 78.0);
        assert_eq!(r.source, Some("latest-rollout"));
        assert_eq!(r.rollout_path.as_deref(), Some(latest.as_path()));
    }

    // -----------------------------------------------------------------------
    // Claude sensor tests
    //
    // All env-mutating (AI_HANDOFF_HOME) tests are sequential in ONE #[test]
    // fn to avoid races with other tests in the workspace (mirrors the
    // approach in paths.rs::home_and_layout_paths).
    // -----------------------------------------------------------------------

    fn make_input(
        session_id: Option<&str>,
        used_percentage: Option<serde_json::Value>,
        resets_at: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let mut obj = serde_json::json!({});
        if let Some(sid) = session_id {
            obj["session_id"] = serde_json::Value::String(sid.to_string());
        }
        let mut fh = serde_json::json!({});
        if let Some(up) = used_percentage {
            fh["used_percentage"] = up;
        }
        if let Some(ra) = resets_at {
            fh["resets_at"] = ra;
        }
        obj["rate_limits"] = serde_json::json!({ "five_hour": fh });
        obj
    }

    fn make_input_with_weekly(
        session_id: &str,
        five_used: f64,
        weekly_used: f64,
        weekly_resets_at: f64,
    ) -> serde_json::Value {
        serde_json::json!({
            "session_id": session_id,
            "rate_limits": {
                "five_hour": { "used_percentage": five_used },
                "seven_day": {
                    "used_percentage": weekly_used,
                    "resets_at": weekly_resets_at
                }
            }
        })
    }

    #[test]
    fn claude_raw_rate_limits_can_drive_trigger_usage() {
        let now_ms = 1_750_000_000_000;
        let raw = serde_json::json!({
            "rate_limits": {
                "five_hour": {
                    "used_percentage": 78.0,
                    "resets_at": 1_750_001_800.0
                }
            }
        });
        let sample = claude_trigger_usage_from_raw(&raw, now_ms).expect("raw usage");
        assert_eq!(sample.used_percent, 78.0);
        assert_eq!(sample.resets_at_ms, Some(1_750_001_800_000));
        assert!(claude_trigger_usage_from_raw(&serde_json::json!({}), now_ms).is_none());
    }

    #[test]
    fn claude_sensor_all_cases() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let now_ms: i64 = 1_750_000_000_000;

        // --- record rejects: empty session_id ---
        let v = make_input(Some(""), Some(serde_json::json!(50.0)), None);
        assert!(
            !record_claude_rate_limit(&v, now_ms),
            "empty session_id must return false"
        );

        // --- record rejects: missing session_id ---
        let v = make_input(None, Some(serde_json::json!(50.0)), None);
        assert!(
            !record_claude_rate_limit(&v, now_ms),
            "missing session_id must return false"
        );

        // --- record rejects: used_percentage = 101 ---
        let v = make_input(Some("sid-A"), Some(serde_json::json!(101.0)), None);
        assert!(
            !record_claude_rate_limit(&v, now_ms),
            "used_percentage=101 must return false"
        );

        // --- record rejects: used_percentage = -1 ---
        let v = make_input(Some("sid-A"), Some(serde_json::json!(-1.0)), None);
        assert!(
            !record_claude_rate_limit(&v, now_ms),
            "used_percentage=-1 must return false"
        );

        // --- record rejects: NaN ---
        let v = make_input(Some("sid-A"), Some(serde_json::json!(f64::NAN)), None);
        assert!(
            !record_claude_rate_limit(&v, now_ms),
            "NaN used_percentage must return false"
        );

        // Verify nothing was written for any of those rejections.
        let dir = crate::paths::rate_limits_dir();
        let count = if dir.exists() {
            std::fs::read_dir(&dir).unwrap().count()
        } else {
            0
        };
        assert_eq!(count, 0, "no files should be written on rejection");

        // --- record then read returns the same used_percent ---
        let v = make_input(Some("sid-roundtrip"), Some(serde_json::json!(42.5)), None);
        assert!(record_claude_rate_limit(&v, now_ms));
        let usage = read_claude_rate_limit(60_000, now_ms).expect("should read back the sample");
        assert_eq!(usage.used_percent, 42.5);
        assert_eq!(usage.window_minutes, 300);
        assert_eq!(usage.source, "claude-statusline");
        assert_eq!(usage.captured_at, now_ms);
        assert!(usage.resets_at.is_none());
        assert!(usage.weekly.is_none());

        let v = make_input_with_weekly("sid-weekly", 12.0, 34.0, 1_750_700_000.0);
        assert!(record_claude_rate_limit(&v, now_ms + 500));
        let usage = read_claude_rate_limit(60_000, now_ms + 500).expect("weekly sample");
        let weekly = usage.weekly.expect("weekly window");
        assert_eq!(weekly.used_percent, 34.0);
        assert_eq!(weekly.window_minutes, 10080);
        assert_eq!(weekly.resets_at, Some(1_750_700_000.0));
        let _ = std::fs::remove_file(
            crate::paths::rate_limits_dir().join(format!("{}.json", sha256_hex("sid-weekly"))),
        );

        // --- resets_at in the past → read returns None ---
        // Write a new session with resets_at 1 second in the past (unix seconds).
        let past_resets = (now_ms as f64 / 1000.0) - 1.0; // 1 second ago
        let v = make_input(
            Some("sid-expired"),
            Some(serde_json::json!(30.0)),
            Some(serde_json::json!(past_resets)),
        );
        assert!(record_claude_rate_limit(&v, now_ms));
        // The only valid session is sid-roundtrip (no resets_at). The expired one is skipped.
        let usage = read_claude_rate_limit(60_000, now_ms).unwrap();
        // It should be from sid-roundtrip, not sid-expired (which has expired resets_at)
        assert_eq!(usage.used_percent, 42.5);

        // --- captured_at older than freshness_ms → None ---
        // Overwrite with a sample whose home is fresh but simulate a stale reading:
        // write a stale sample for a NEW session (won't overwrite sid-roundtrip).
        let stale_time = now_ms - 120_001; // older than 120_000 ms freshness
        let v = make_input(Some("sid-stale"), Some(serde_json::json!(99.0)), None);
        assert!(record_claude_rate_limit(&v, stale_time));
        // read with freshness_ms=120_000 — sid-stale is too old, sid-roundtrip still valid.
        let usage = read_claude_rate_limit(120_000, now_ms).unwrap();
        assert!(
            (usage.used_percent - 42.5).abs() < f64::EPSILON,
            "stale sample should be ignored, got {}",
            usage.used_percent
        );

        // --- two samples, freshest captured_at wins ---
        // Write a fresher sample for a new session.
        let fresher_time = now_ms + 1000;
        let v = make_input(Some("sid-fresh"), Some(serde_json::json!(77.0)), None);
        assert!(record_claude_rate_limit(&v, fresher_time));
        let usage = read_claude_rate_limit(120_000, fresher_time + 1000).unwrap();
        assert_eq!(usage.used_percent, 77.0, "freshest sample should win");
        assert_eq!(usage.captured_at, fresher_time);

        // --- garbage file in dir is skipped, valid one still read ---
        let garbage = crate::paths::rate_limits_dir().join("garbage.json");
        std::fs::write(&garbage, b"NOT JSON AT ALL!!!").unwrap();
        // Should still read the valid sample.
        let usage = read_claude_rate_limit(120_000, fresher_time + 1000);
        assert!(
            usage.is_some(),
            "garbage file should be skipped, valid sample still returned"
        );

        std::env::remove_var("AI_HANDOFF_HOME");

        // --- trigger helper: stale sample with an open window still counts ---
        {
            let trigger_home = tempfile::tempdir().unwrap();
            std::env::set_var("AI_HANDOFF_HOME", trigger_home.path());
            let now_ms: i64 = 1_750_000_000_000;
            let future_reset = (now_ms as f64 / 1000.0) + 3600.0;

            // 2h-old sample, resets_at 1h in the future → monotonic lower
            // bound, usable despite being far past the strict freshness.
            let stale_open = make_input(
                Some("sid-trigger-open"),
                Some(serde_json::json!(83.0)),
                Some(serde_json::json!(future_reset)),
            );
            assert!(record_claude_rate_limit(
                &stale_open,
                now_ms - 2 * 60 * 60 * 1000
            ));
            assert_eq!(claude_used_percent_for_trigger(now_ms), Some(83.0));
            let sample = claude_trigger_usage(now_ms).expect("trigger sample");
            assert_eq!(sample.used_percent, 83.0);
            assert_eq!(sample.resets_at_ms, Some((future_reset * 1000.0) as i64));

            // Without resets_at the same age is NOT trusted...
            let trigger_home2 = tempfile::tempdir().unwrap();
            std::env::set_var("AI_HANDOFF_HOME", trigger_home2.path());
            let no_reset = make_input(
                Some("sid-trigger-bare"),
                Some(serde_json::json!(90.0)),
                None,
            );
            assert!(record_claude_rate_limit(
                &no_reset,
                now_ms - 2 * 60 * 60 * 1000
            ));
            assert_eq!(claude_used_percent_for_trigger(now_ms), None);

            // ...but a recent capture without resets_at is.
            let trigger_home3 = tempfile::tempdir().unwrap();
            std::env::set_var("AI_HANDOFF_HOME", trigger_home3.path());
            let fresh_bare = make_input(
                Some("sid-trigger-fresh"),
                Some(serde_json::json!(70.0)),
                None,
            );
            assert!(record_claude_rate_limit(&fresh_bare, now_ms - 60_000));
            assert_eq!(claude_used_percent_for_trigger(now_ms), Some(70.0));
            let sample = claude_trigger_usage(now_ms).expect("recent trigger sample");
            assert_eq!(sample.used_percent, 70.0);
            assert_eq!(sample.resets_at_ms, None);

            // An expired window yields nothing (read layer drops it).
            let trigger_home4 = tempfile::tempdir().unwrap();
            std::env::set_var("AI_HANDOFF_HOME", trigger_home4.path());
            let expired = make_input(
                Some("sid-trigger-expired"),
                Some(serde_json::json!(99.0)),
                Some(serde_json::json!((now_ms as f64 / 1000.0) - 1.0)),
            );
            assert!(record_claude_rate_limit(&expired, now_ms - 60_000));
            assert_eq!(claude_used_percent_for_trigger(now_ms), None);
            std::env::remove_var("AI_HANDOFF_HOME");
        }

        // --- missing dir → read returns None ---
        // Point AI_HANDOFF_HOME to a directory with no rate-limits subdir.
        let empty_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", empty_home.path());
        let result = read_claude_rate_limit(60_000, now_ms);
        assert!(result.is_none(), "missing dir must return None");

        std::env::remove_var("AI_HANDOFF_HOME");
    }
}
