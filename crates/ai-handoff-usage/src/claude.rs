//! Claude parser: `~/.claude/projects/<encoded>/<session>.jsonl`.
//!
//! Each line is one transcript entry. Assistant turns carry `message.usage`
//! with cache-exclusive `input_tokens` plus separate `cache_read_input_tokens`
//! and `cache_creation_input_tokens`. We keep one event per `message.id`
//! (deduped across files, since the same assistant message can be re-logged on
//! compaction); lines without an id fall back to a path+line content hash so no
//! usage is silently dropped. Malformed lines are skipped.

use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::model::{local_day, Source, Tokens, UsageEvent};

/// Parse one Claude session file, appending [`UsageEvent`]s to `out` and
/// recording dedupe keys in `seen` (shared across files). A missing file or an
/// unreadable line is skipped, never fatal.
pub fn parse_file(
    path: &Path,
    seen: &mut HashSet<String>,
    out: &mut Vec<UsageEvent>,
) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let path_str = path.to_string_lossy();
    for (lineno, line) in reader.lines().enumerate() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(ev) = parse_line(&line, &path_str, lineno, seen) {
            out.push(ev);
        }
    }
    Ok(())
}

fn parse_line(
    line: &str,
    path: &str,
    lineno: usize,
    seen: &mut HashSet<String>,
) -> Option<UsageEvent> {
    let v: Value = serde_json::from_str(line).ok()?;
    let msg = v.get("message")?;
    let usage = msg.get("usage")?;

    let key = match msg.get("id").and_then(Value::as_str) {
        Some(id) => hash(&["claude", id]),
        None => hash(&["claude-fallback", path, &lineno.to_string(), line]),
    };
    if !seen.insert(key) {
        return None; // already counted
    }

    let model = msg
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let project = v
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let session = v
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let day = v
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(local_day)
        .unwrap_or_default();

    let u = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let tokens = Tokens {
        input: u("input_tokens"),
        cache_read: u("cache_read_input_tokens"),
        cache_write: u("cache_creation_input_tokens"),
        output: u("output_tokens"),
    };

    Some(UsageEvent {
        source: Source::Claude,
        project,
        session,
        model,
        day,
        tokens,
    })
}

fn hash(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0u8]); // separator so concatenation is unambiguous
    }
    format!("{:x}", h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_line(id: &str, model: &str, input: u64, cr: u64, cw: u64, out: u64) -> String {
        format!(
            r#"{{"type":"assistant","cwd":"C:/proj","sessionId":"sess1","timestamp":"2026-06-17T14:12:08.827Z","message":{{"id":"{id}","model":"{model}","usage":{{"input_tokens":{input},"cache_read_input_tokens":{cr},"cache_creation_input_tokens":{cw},"output_tokens":{out}}}}}}}"#
        )
    }

    fn parse_str(text: &str) -> Vec<UsageEvent> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(&path, text).unwrap();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        parse_file(&path, &mut seen, &mut out).unwrap();
        out
    }

    #[test]
    fn parses_assistant_usage_into_normalized_event() {
        let events = parse_str(&assistant_line(
            "msg_1",
            "claude-opus-4-8",
            2,
            0,
            39459,
            245,
        ));
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.source, Source::Claude);
        assert_eq!(e.model, "claude-opus-4-8");
        assert_eq!(e.project, "C:/proj");
        assert_eq!(e.session, "sess1");
        assert_eq!(
            e.tokens,
            Tokens {
                input: 2,
                cache_read: 0,
                cache_write: 39459,
                output: 245
            }
        );
    }

    #[test]
    fn dedupes_repeated_message_id() {
        let line = assistant_line("dup", "claude-opus-4-8", 10, 0, 0, 5);
        let text = format!("{line}\n{line}\n");
        let events = parse_str(&text);
        assert_eq!(events.len(), 1, "same message.id must count once");
    }

    #[test]
    fn skips_user_and_malformed_lines() {
        let text = format!(
            "{}\n{}\n{}\n",
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
            "this is not json",
            assistant_line("real", "claude-sonnet-4-6", 100, 50, 0, 20),
        );
        let events = parse_str(&text);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].model, "claude-sonnet-4-6");
    }

    #[test]
    fn missing_id_falls_back_and_still_counts() {
        let no_id = r#"{"cwd":"C:/p","timestamp":"2026-06-17T14:12:08.827Z","message":{"model":"claude-opus-4-8","usage":{"input_tokens":7,"output_tokens":3}}}"#;
        let events = parse_str(no_id);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tokens.input, 7);
        assert_eq!(events[0].tokens.output, 3);
    }

    #[test]
    fn missing_file_is_not_fatal() {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let res = parse_file(
            Path::new("C:/nope/does-not-exist.jsonl"),
            &mut seen,
            &mut out,
        );
        assert!(res.is_err()); // open failed, but no panic
        assert!(out.is_empty());
    }
}
