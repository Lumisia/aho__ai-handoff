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

/// Whether an outer hook entry (`{"matcher": …, "hooks": [...]}`) is one of
/// ours, i.e. any of its inner hooks is managed for `agent`.
pub fn outer_entry_is_managed(outer: &Value, agent: &str) -> bool {
    outer
        .get("hooks")
        .and_then(Value::as_array)
        .map(|inner| inner.iter().any(|hook| inner_hook_is_managed(hook, agent)))
        .unwrap_or(false)
}

/// Whether a single inner hook object is one of ours.
///
/// Matches on the `_aiHandoff` marker when present, otherwise on the
/// invocation. Both the exec form Claude uses (`command` = the bare exe,
/// `args` = the CLI words) and the string form Codex uses (the whole
/// invocation inside `command`) are recognised.
pub fn inner_hook_is_managed(hook: &Value, agent: &str) -> bool {
    if hook.get("_aiHandoff").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    let Some(command) = hook.get("command").and_then(Value::as_str) else {
        return false;
    };
    let tokens = match hook.get("args").and_then(Value::as_array) {
        Some(args) => {
            let mut tokens = vec![command.to_string()];
            tokens.extend(args.iter().filter_map(Value::as_str).map(str::to_string));
            tokens
        }
        None => split_command_line(command),
    };
    tokens_are_managed(&tokens, agent)
}

/// Whether any event array under a `hooks` object holds a managed entry.
///
/// `value` is the event array itself (`hooks.<Event>`), which may be absent.
pub fn event_has_managed_hook(value: Option<&Value>, agent: &str) -> bool {
    value
        .and_then(Value::as_array)
        .map(|outer| {
            outer
                .iter()
                .any(|entry| outer_entry_is_managed(entry, agent))
        })
        .unwrap_or(false)
}

/// `["<exe>", "hook", "<event-arg>", "--agent", "<agent>"]` — our invocation
/// and nothing else.
fn tokens_are_managed(tokens: &[String], agent: &str) -> bool {
    let Some(program) = tokens.first() else {
        return false;
    };
    if !is_ai_handoff_exe(program) {
        return false;
    }
    let rest = &tokens[1..];
    if rest.first().map(String::as_str) != Some("hook") {
        return false;
    }
    rest.windows(2)
        .any(|pair| pair[0] == "--agent" && pair[1] == agent)
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
    for ch in text.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
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
        assert!(inner_hook_is_managed(&claude_exec_hook(true), AGENT_CLAUDE));
        assert!(inner_hook_is_managed(&codex_string_hook(true), AGENT_CODEX));
    }

    #[test]
    fn unmarked_hooks_are_still_recognised_by_invocation() {
        assert!(inner_hook_is_managed(
            &claude_exec_hook(false),
            AGENT_CLAUDE
        ));
        assert!(inner_hook_is_managed(
            &codex_string_hook(false),
            AGENT_CODEX
        ));
    }

    #[test]
    fn agent_slug_must_match() {
        assert!(!inner_hook_is_managed(
            &claude_exec_hook(false),
            AGENT_CODEX
        ));
        assert!(!inner_hook_is_managed(
            &codex_string_hook(false),
            AGENT_CLAUDE
        ));
    }

    #[test]
    fn marker_wins_even_when_the_agent_slug_differs() {
        // A marked entry is ours by construction; we wrote it.
        assert!(inner_hook_is_managed(&claude_exec_hook(true), AGENT_CODEX));
    }

    #[test]
    fn foreign_hooks_are_not_managed() {
        let plain = json!({"type": "command", "command": "other-tool"});
        assert!(!inner_hook_is_managed(&plain, AGENT_CLAUDE));

        // Same binary, but not our `hook` subcommand — a user's own wiring.
        let user_owned = json!({
            "type": "command",
            "command": "\"C:\\bin\\ai-handoff.exe\" statusline"
        });
        assert!(!inner_hook_is_managed(&user_owned, AGENT_CLAUDE));

        // Sibling binary must not be swallowed.
        let host = json!({
            "type": "command",
            "command": "\"C:\\bin\\ai-handoff-host.exe\" hook stop --agent claude-code"
        });
        assert!(!inner_hook_is_managed(&host, AGENT_CLAUDE));

        // No command at all.
        assert!(!inner_hook_is_managed(
            &json!({"type": "command"}),
            AGENT_CLAUDE
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
        assert!(outer_entry_is_managed(&outer, AGENT_CLAUDE));

        let event = json!([
            {"hooks": [{"type": "command", "command": "other"}]},
            outer
        ]);
        assert!(event_has_managed_hook(Some(&event), AGENT_CLAUDE));
        assert!(!event_has_managed_hook(Some(&event), AGENT_CODEX));
        assert!(!event_has_managed_hook(None, AGENT_CLAUDE));
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
