//! Codex parser: `~/.codex/sessions/**` and `~/.codex/archived_sessions/**`.
//!
//! A rollout file is a stream of `{timestamp, type, payload}` records:
//! - `session_meta` (first line) → `payload.{id, cwd}` (session + project).
//! - `turn_context` → `payload.{model, cwd}` (current model for following turns).
//! - `event_msg` with `payload.type == "token_count"` →
//!   `payload.info.last_token_usage`, the per-turn delta (verified: the sum of
//!   `last_token_usage` equals the final `total_token_usage`, so summing deltas
//!   never double-counts).
//!
//! Codex `input_tokens` **includes** `cached_input_tokens`, so fresh input is
//! `input_tokens - cached_input_tokens`; there is no cache-creation bucket and
//! `output_tokens` already includes reasoning tokens.

use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::model::{local_day, Source, Tokens, UsageEvent};

/// Parse one Codex rollout file, appending one [`UsageEvent`] per `token_count`
/// delta to `out`. A missing file or unreadable line is skipped, never fatal.
pub fn parse_file(path: &Path, out: &mut Vec<UsageEvent>) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);

    let mut model = String::from("unknown");
    let mut project = String::new();
    let mut session = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let payload = v.get("payload");
        match v.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                if let Some(p) = payload {
                    if let Some(cwd) = p.get("cwd").and_then(Value::as_str) {
                        project = cwd.to_string();
                    }
                    if let Some(id) = p.get("id").and_then(Value::as_str) {
                        session = id.to_string();
                    }
                }
            }
            Some("turn_context") => {
                if let Some(p) = payload {
                    if let Some(m) = p.get("model").and_then(Value::as_str) {
                        model = m.to_string();
                    }
                    if let Some(cwd) = p.get("cwd").and_then(Value::as_str) {
                        project = cwd.to_string();
                    }
                }
            }
            _ => {
                if let Some(tokens) = token_count_delta(payload) {
                    let day = v
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .and_then(local_day)
                        .unwrap_or_default();
                    out.push(UsageEvent {
                        source: Source::Codex,
                        project: project.clone(),
                        session: session.clone(),
                        model: model.clone(),
                        day,
                        tokens,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Extract normalized per-turn tokens from a `token_count` payload, or `None`
/// if this payload is not a token_count event.
fn token_count_delta(payload: Option<&Value>) -> Option<Tokens> {
    let payload = payload?;
    if payload.get("type").and_then(Value::as_str)? != "token_count" {
        return None;
    }
    let last = payload.get("info")?.get("last_token_usage")?;
    let u = |k: &str| last.get(k).and_then(Value::as_u64).unwrap_or(0);
    let input_total = u("input_tokens");
    let cached = u("cached_input_tokens");
    Some(Tokens {
        input: input_total.saturating_sub(cached),
        cache_read: cached,
        cache_write: 0,
        output: u("output_tokens"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(text: &str) -> Vec<UsageEvent> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-x.jsonl");
        std::fs::write(&path, text).unwrap();
        let mut out = Vec::new();
        parse_file(&path, &mut out).unwrap();
        out
    }

    fn token_count(ts: &str, input: u64, cached: u64, output: u64) -> String {
        format!(
            r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output},"total_tokens":{}}}}}}}}}"#,
            input + output
        )
    }

    const META: &str = r#"{"timestamp":"2026-06-17T14:12:01Z","type":"session_meta","payload":{"id":"sess-abc","cwd":"C:/proj"}}"#;
    const CTX: &str = r#"{"timestamp":"2026-06-17T14:12:02Z","type":"turn_context","payload":{"model":"gpt-5.5","cwd":"C:/proj"}}"#;

    #[test]
    fn attributes_tokens_to_current_model_and_project() {
        let text = format!(
            "{META}\n{CTX}\n{}\n",
            token_count("2026-06-17T14:12:08Z", 14881, 7040, 147)
        );
        let events = parse_str(&text);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.source, Source::Codex);
        assert_eq!(e.model, "gpt-5.5");
        assert_eq!(e.project, "C:/proj");
        assert_eq!(e.session, "sess-abc");
        // input_tokens includes cached: fresh = 14881 - 7040.
        assert_eq!(e.tokens, Tokens { input: 7841, cache_read: 7040, cache_write: 0, output: 147 });
    }

    #[test]
    fn sums_multiple_turn_deltas_without_double_counting() {
        let text = format!(
            "{META}\n{CTX}\n{}\n{}\n",
            token_count("2026-06-17T14:12:08Z", 100, 0, 10),
            token_count("2026-06-17T14:13:00Z", 200, 40, 20),
        );
        let events = parse_str(&text);
        assert_eq!(events.len(), 2);
        let total: u64 = events.iter().map(|e| e.tokens.total()).sum();
        // (100+10) + ((200-40)+40+20) = 110 + 220 = 330
        assert_eq!(total, 330);
    }

    #[test]
    fn model_switch_reattributes_following_turns() {
        let ctx2 = r#"{"timestamp":"2026-06-17T14:20:00Z","type":"turn_context","payload":{"model":"gpt-5-codex"}}"#;
        let text = format!(
            "{META}\n{CTX}\n{}\n{ctx2}\n{}\n",
            token_count("2026-06-17T14:12:08Z", 10, 0, 1),
            token_count("2026-06-17T14:21:00Z", 20, 0, 2),
        );
        let events = parse_str(&text);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].model, "gpt-5.5");
        assert_eq!(events[1].model, "gpt-5-codex");
    }

    #[test]
    fn token_count_before_any_context_uses_unknown_model() {
        let text = format!("{}\n", token_count("2026-06-17T14:12:08Z", 5, 0, 1));
        let events = parse_str(&text);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].model, "unknown");
    }

    #[test]
    fn non_token_events_and_garbage_are_skipped() {
        let text = format!(
            "{META}\n{}\n{}\nnot json\n",
            r#"{"timestamp":"2026-06-17T14:12:05Z","type":"event_msg","payload":{"type":"task_started"}}"#,
            token_count("2026-06-17T14:12:08Z", 5, 0, 1),
        );
        let events = parse_str(&text);
        assert_eq!(events.len(), 1);
    }
}
