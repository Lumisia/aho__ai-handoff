//! Roots discovery and the in-process scan that ties the parsers together.
//!
//! `scan` walks the Claude and Codex log roots, parses every `*.jsonl`, and
//! returns the combined, deduped [`UsageEvent`] list. Everything is read-only
//! and best-effort: missing roots yield no events, unreadable files are
//! skipped. The Claude dedupe set is shared across all Claude files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::model::UsageEvent;
use crate::{claude, codex};

/// The log directories to scan.
#[derive(Debug, Clone, Default)]
pub struct Roots {
    /// `~/.claude/projects` (or `None` if the home dir is unknown).
    pub claude_projects: Option<PathBuf>,
    /// Codex log dirs: `<CODEX_HOME>/sessions` and `<CODEX_HOME>/archived_sessions`.
    pub codex_dirs: Vec<PathBuf>,
}

/// Resolve the default roots from the user's home dir, honoring `CODEX_HOME`.
pub fn default_roots() -> Roots {
    let home = home_dir();
    let claude_projects = home.as_ref().map(|h| h.join(".claude").join("projects"));
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".codex")));
    let codex_dirs = codex_home
        .map(|c| vec![c.join("sessions"), c.join("archived_sessions")])
        .unwrap_or_default();
    Roots {
        claude_projects,
        codex_dirs,
    }
}

/// Scan all roots and return the combined usage events.
pub fn scan(roots: &Roots) -> Vec<UsageEvent> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    if let Some(root) = &roots.claude_projects {
        for file in jsonl_files(root) {
            let _ = claude::parse_file(&file, &mut seen, &mut out);
        }
    }
    for dir in &roots.codex_dirs {
        for file in jsonl_files(dir) {
            let _ = codex::parse_file(&file, &mut out);
        }
    }
    out
}

/// Convenience: scan the default roots.
pub fn scan_default() -> Vec<UsageEvent> {
    scan(&default_roots())
}

/// Recursively collect every `*.jsonl` file under `root` (returns empty when
/// `root` is missing or unreadable). Iterative to avoid deep-recursion limits.
fn jsonl_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
                found.push(path);
            }
        }
    }
    found
}

fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Source;

    #[test]
    fn scan_walks_nested_dirs_for_both_agents() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join(".claude/projects/proj-enc");
        let codex = dir.path().join(".codex/sessions/2026/06/17");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::create_dir_all(&codex).unwrap();

        std::fs::write(
            claude.join("s.jsonl"),
            r#"{"cwd":"C:/p","timestamp":"2026-06-17T14:00:00Z","message":{"id":"m1","model":"claude-opus-4-8","usage":{"input_tokens":10,"output_tokens":2}}}"#,
        )
        .unwrap();
        std::fs::write(
            codex.join("rollout-x.jsonl"),
            format!(
                "{}\n{}\n",
                r#"{"timestamp":"2026-06-17T14:00:00Z","type":"turn_context","payload":{"model":"gpt-5.5","cwd":"C:/p"}}"#,
                r#"{"timestamp":"2026-06-17T14:00:05Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20,"cached_input_tokens":0,"output_tokens":3,"total_tokens":23}}}}"#,
            ),
        )
        .unwrap();

        let roots = Roots {
            claude_projects: Some(dir.path().join(".claude/projects")),
            codex_dirs: vec![dir.path().join(".codex/sessions")],
        };
        let events = scan(&roots);
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|e| e.source == Source::Claude && e.tokens.input == 10));
        assert!(events.iter().any(|e| e.source == Source::Codex && e.model == "gpt-5.5"));
    }

    #[test]
    fn missing_roots_yield_no_events() {
        let roots = Roots {
            claude_projects: Some(PathBuf::from("C:/nope/claude")),
            codex_dirs: vec![PathBuf::from("C:/nope/codex")],
        };
        assert!(scan(&roots).is_empty());
    }

    #[test]
    fn default_roots_honor_codex_home() {
        std::env::set_var("CODEX_HOME", "C:/custom/codex");
        let roots = default_roots();
        assert!(roots
            .codex_dirs
            .iter()
            .any(|p| p.starts_with("C:/custom/codex")));
        std::env::remove_var("CODEX_HOME");
    }
}
