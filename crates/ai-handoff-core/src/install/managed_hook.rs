//! Identify hook entries that ai-handoff owns.
//!
//! Ownership used to be decided purely by the `_aiHandoff: true` marker we
//! stamp onto every inner hook we write. That marker is not part of the host's
//! hook schema, so a host that reloads and re-serializes its settings file can
//! drop it while leaving the hook itself intact. Observed in the wild on
//! `~/.claude/settings.json`: the only delta between two snapshots was four
//! deleted `_aiHandoff` lines.
//!
//! Once the marker is gone, marker-only matching misfires three ways:
//! `apply` treats our own entry as foreign and appends a second copy, `remove`
//! leaves the unmarked copy behind, and the doctor reports the hooks as
//! missing — which nudges the user into another install, adding yet another
//! copy.
//!
//! So ownership is decided by the marker **or** by the invocation itself. The
//! invocation match is deliberately narrow — program basename `ai-handoff`,
//! first argument `hook`, and an `--agent <agent>` pair for the agent being
//! serviced — so a user's own hook that happens to shell out to our binary is
//! still treated as foreign.

use serde_json::Value;

/// Agent slug carried by Claude hooks (`--agent claude-code`).
pub const AGENT_CLAUDE: &str = "claude-code";

/// Agent slug carried by Codex hooks (`--agent codex`).
pub const AGENT_CODEX: &str = "codex";

/// Host lifecycle event names and their matching `ai-handoff hook` arguments.
pub const MANAGED_EVENTS: [(&str, &str); 4] = [
    ("SessionStart", "session-start"),
    ("UserPromptSubmit", "user-prompt"),
    ("PostToolUse", "post-tool-use"),
    ("Stop", "stop"),
];

/// Whether an outer hook entry (`{"matcher": …, "hooks": [...]}`) is one of
/// ours, i.e. any of its inner hooks is managed for `agent`.
pub fn outer_entry_is_managed(outer: &Value, agent: &str, event_arg: &str) -> bool {
    outer
        .get("hooks")
        .and_then(Value::as_array)
        .map(|inner| {
            inner
                .iter()
                .any(|hook| inner_hook_is_managed(hook, agent, event_arg))
        })
        .unwrap_or(false)
}

/// Return an outer entry with only this event's managed inner hooks removed.
///
/// Outer metadata and foreign inner hooks are preserved byte-for-byte at the
/// JSON value level. An outer entry is dropped only when at least one managed
/// hook was removed and no inner hooks remain.
pub fn without_managed_inner_hooks(outer: &Value, agent: &str, event_arg: &str) -> Option<Value> {
    let Some(inner) = outer.get("hooks").and_then(Value::as_array) else {
        return Some(outer.clone());
    };

    let mut removed = false;
    let kept: Vec<Value> = inner
        .iter()
        .filter_map(|hook| {
            if inner_hook_is_managed(hook, agent, event_arg) {
                removed = true;
                None
            } else {
                Some(hook.clone())
            }
        })
        .collect();

    if !removed {
        return Some(outer.clone());
    }
    if kept.is_empty() {
        return None;
    }

    let mut cleaned = outer.clone();
    cleaned["hooks"] = Value::Array(kept);
    Some(cleaned)
}

/// Whether a single inner hook object is one of ours.
///
/// Matches on the `_aiHandoff` marker when present, otherwise on the
/// invocation. Both the exec form Claude uses (`command` = the bare exe,
/// `args` = the CLI words) and the string form Codex uses (the whole
/// invocation inside `command`) are recognised.
pub fn inner_hook_is_managed(hook: &Value, agent: &str, event_arg: &str) -> bool {
    if hook.get("_aiHandoff").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    if hook.get("type").and_then(Value::as_str) != Some("command") {
        return false;
    }
    let Some(command) = hook.get("command").and_then(Value::as_str) else {
        return false;
    };
    let tokens = match hook.get("args").and_then(Value::as_array) {
        Some(args) => {
            let Some(args) = args.iter().map(Value::as_str).collect::<Option<Vec<_>>>() else {
                return false;
            };
            let mut tokens = vec![command.to_string()];
            tokens.extend(args.into_iter().map(str::to_string));
            tokens
        }
        None => split_command_line(command),
    };
    tokens_are_managed(&tokens, agent, event_arg)
}

/// Whether any event array under a `hooks` object holds a managed entry.
///
/// `value` is the event array itself (`hooks.<Event>`), which may be absent.
pub fn event_has_managed_hook(value: Option<&Value>, agent: &str, event_arg: &str) -> bool {
    value
        .and_then(Value::as_array)
        .map(|outer| {
            outer
                .iter()
                .any(|entry| outer_entry_is_managed(entry, agent, event_arg))
        })
        .unwrap_or(false)
}

/// `["<exe>", "hook", "<event-arg>", "--agent", "<agent>"]` — our invocation
/// and nothing else.
fn tokens_are_managed(tokens: &[String], agent: &str, event_arg: &str) -> bool {
    tokens.len() == 5
        && is_ai_handoff_exe(&tokens[0])
        && tokens[1] == "hook"
        && tokens[2] == event_arg
        && tokens[3] == "--agent"
        && tokens[4] == agent
}

/// Basename match on the program token, ignoring path separators, a `.exe`
/// suffix and case. Sibling binaries such as `ai-handoff-host.exe` do NOT
/// match — the stem must be exactly `ai-handoff`.
fn is_ai_handoff_exe(program: &str) -> bool {
    let normalized = program.replace('\\', "/");
    let file = normalized.rsplit('/').next().unwrap_or_default();
    let stem = match file.len().checked_sub(4) {
        Some(cut) if file[cut..].eq_ignore_ascii_case(".exe") => &file[..cut],
        _ => file,
    };
    stem.eq_ignore_ascii_case("ai-handoff")
}

/// Split a command string into tokens, treating a double-quoted run as one
/// token. Enough for the `"<exe>" hook <event> --agent <agent>` form we write;
/// this is not a general shell parser.
fn split_command_line(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut token_started = false;
    for ch in text.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                token_started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if token_started {
                    tokens.push(std::mem::take(&mut current));
                    token_started = false;
                }
            }
            c => {
                current.push(c);
                token_started = true;
            }
        }
    }
    if token_started {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn claude_exec_hook(marker: bool) -> Value {
        let mut hook = json!({
            "type": "command",
            "command": "C:\\Users\\PC\\.ai-handoff\\bin\\ai-handoff.exe",
            "args": ["hook", "stop", "--agent", "claude-code"],
            "timeout": 10
        });
        if marker {
            hook["_aiHandoff"] = json!(true);
        }
        hook
    }

    fn codex_string_hook(marker: bool) -> Value {
        let mut hook = json!({
            "type": "command",
            "command": "\"C:\\Users\\PC\\.ai-handoff\\bin\\ai-handoff.exe\" hook stop --agent codex",
            "timeout": 10
        });
        if marker {
            hook["_aiHandoff"] = json!(true);
        }
        hook
    }

    #[test]
    fn marked_hooks_are_managed() {
        assert!(inner_hook_is_managed(
            &claude_exec_hook(true),
            AGENT_CLAUDE,
            "stop"
        ));
        assert!(inner_hook_is_managed(
            &codex_string_hook(true),
            AGENT_CODEX,
            "stop"
        ));
    }

    #[test]
    fn unmarked_hooks_are_still_recognised_by_invocation() {
        assert!(inner_hook_is_managed(
            &claude_exec_hook(false),
            AGENT_CLAUDE,
            "stop"
        ));
        assert!(inner_hook_is_managed(
            &codex_string_hook(false),
            AGENT_CODEX,
            "stop"
        ));
    }

    #[test]
    fn unmarked_invocation_must_match_the_current_event() {
        assert!(!inner_hook_is_managed(
            &claude_exec_hook(false),
            AGENT_CLAUDE,
            "session-start"
        ));
        assert!(!inner_hook_is_managed(
            &codex_string_hook(false),
            AGENT_CODEX,
            "post-tool-use"
        ));
    }

    #[test]
    fn agent_slug_must_match() {
        assert!(!inner_hook_is_managed(
            &claude_exec_hook(false),
            AGENT_CODEX,
            "stop"
        ));
        assert!(!inner_hook_is_managed(
            &codex_string_hook(false),
            AGENT_CLAUDE,
            "stop"
        ));
    }

    #[test]
    fn marker_wins_even_when_the_agent_slug_differs() {
        // A marked entry is ours by construction; we wrote it.
        assert!(inner_hook_is_managed(
            &claude_exec_hook(true),
            AGENT_CODEX,
            "session-start"
        ));
    }

    #[test]
    fn foreign_hooks_are_not_managed() {
        let plain = json!({"type": "command", "command": "other-tool"});
        assert!(!inner_hook_is_managed(&plain, AGENT_CLAUDE, "stop"));

        // Same binary, but not our `hook` subcommand — a user's own wiring.
        let user_owned = json!({
            "type": "command",
            "command": "\"C:\\bin\\ai-handoff.exe\" statusline"
        });
        assert!(!inner_hook_is_managed(&user_owned, AGENT_CLAUDE, "stop"));

        // Sibling binary must not be swallowed.
        let host = json!({
            "type": "command",
            "command": "\"C:\\bin\\ai-handoff-host.exe\" hook stop --agent claude-code"
        });
        assert!(!inner_hook_is_managed(&host, AGENT_CLAUDE, "stop"));

        // No command at all.
        assert!(!inner_hook_is_managed(
            &json!({"type": "command"}),
            AGENT_CLAUDE,
            "stop"
        ));
    }

    #[test]
    fn unmarked_invocation_rejects_extra_tokens_and_wrong_type() {
        let extra = json!({
            "type": "command",
            "command": "C:\\bin\\ai-handoff.exe",
            "args": ["hook", "stop", "--verbose", "--agent", "claude-code"]
        });
        let wrong_type = json!({
            "type": "shell",
            "command": "C:\\bin\\ai-handoff.exe",
            "args": ["hook", "stop", "--agent", "claude-code"]
        });
        let non_string_arg = json!({
            "type": "command",
            "command": "C:\\bin\\ai-handoff.exe",
            "args": ["hook", "stop", 7, "--agent", "claude-code"]
        });
        let empty_quoted_extra = json!({
            "type": "command",
            "command": "\"C:\\bin\\ai-handoff.exe\" hook stop \"\" --agent codex"
        });

        assert!(!inner_hook_is_managed(&extra, AGENT_CLAUDE, "stop"));
        assert!(!inner_hook_is_managed(&wrong_type, AGENT_CLAUDE, "stop"));
        assert!(!inner_hook_is_managed(
            &non_string_arg,
            AGENT_CLAUDE,
            "stop"
        ));
        assert!(!inner_hook_is_managed(
            &empty_quoted_extra,
            AGENT_CODEX,
            "stop"
        ));
    }

    #[test]
    fn exe_basename_matching_tolerates_path_and_case() {
        assert!(is_ai_handoff_exe(
            "C:\\Users\\PC\\.ai-handoff\\bin\\ai-handoff.exe"
        ));
        assert!(is_ai_handoff_exe("/usr/local/bin/ai-handoff"));
        assert!(is_ai_handoff_exe("AI-Handoff.EXE"));
        assert!(!is_ai_handoff_exe("ai-handoff-host.exe"));
        assert!(!is_ai_handoff_exe("handoff.exe"));
        assert!(!is_ai_handoff_exe(""));
    }

    #[test]
    fn outer_and_event_helpers_walk_the_nesting() {
        let outer = json!({"matcher": "*", "hooks": [claude_exec_hook(false)]});
        assert!(outer_entry_is_managed(&outer, AGENT_CLAUDE, "stop"));

        let event = json!([
            {"hooks": [{"type": "command", "command": "other"}]},
            outer
        ]);
        assert!(event_has_managed_hook(Some(&event), AGENT_CLAUDE, "stop"));
        assert!(!event_has_managed_hook(Some(&event), AGENT_CODEX, "stop"));
        assert!(!event_has_managed_hook(None, AGENT_CLAUDE, "stop"));
    }

    #[test]
    fn split_command_line_keeps_quoted_paths_intact() {
        let tokens =
            split_command_line("\"C:\\Program Files\\ai-handoff.exe\" hook stop --agent codex");
        assert_eq!(
            tokens,
            vec![
                "C:\\Program Files\\ai-handoff.exe",
                "hook",
                "stop",
                "--agent",
                "codex"
            ]
        );
    }
}
