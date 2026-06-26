//! Install/uninstall our managed Claude Code `statusLine` setting.
//!
//! The `statusLine` key lives at the **root** of `settings.json` (a sibling of
//! `hooks`, which [`super::claude`] owns). These functions only touch
//! `statusLine`; everything else in the document is preserved verbatim.
//!
//! Like [`super::claude`], the JSON helpers are pure: they take the existing
//! text, return `serde_json::Result<...>`, and **Err on a parse failure rather
//! than clobbering** (the backup is the recovery path).

use serde_json::{json, Value};

use super::state::ClaudeStatuslineState;

/// The `statusLine.command` string we install, pointing at our own exe.
pub fn installed_command(exe: &str) -> String {
    format!("\"{exe}\" statusline")
}

/// Outcome of [`apply`], carrying what the caller needs to record into state.
pub struct Apply {
    /// True when the document's pre-existing `statusLine.command` already equalled
    /// [`installed_command`] (i.e. we are re-applying over our own setting).
    pub current_was_ours: bool,
    /// The user's prior `statusLine` value to restore on uninstall. `None` when
    /// there was no prior statusLine, or when it was already ours (the caller
    /// keeps the previously recorded `previous` in that case).
    pub previous: Option<Value>,
    /// The command string we installed (mirrors [`installed_command`]).
    pub installed_command: String,
}

/// Install our managed `statusLine` into `existing` JSON (or a blank object when
/// `None`), leaving every other key untouched.
///
/// Returns `Ok((pretty_json, Apply))`, or a parse error when `existing` contains
/// invalid JSON (caller should abort — never silently clobber).
pub fn apply(existing: Option<&str>, exe: &str) -> serde_json::Result<(String, Apply)> {
    let mut root: Value = match existing {
        Some(s) => serde_json::from_str(s)?,
        None => json!({}),
    };

    let command = installed_command(exe);

    // Snapshot the current statusLine before we overwrite it.
    let current = root.get("statusLine").cloned();
    let current_was_ours = current
        .as_ref()
        .and_then(|sl| sl.get("command"))
        .and_then(Value::as_str)
        == Some(command.as_str());

    // `previous` is what we'd restore on uninstall: nothing when it was already
    // ours (caller keeps the older recorded value), otherwise whatever foreign
    // statusLine existed (or None when there was none at all).
    let previous = if current_was_ours { None } else { current };

    root["statusLine"] = json!({
        "type": "command",
        "command": command,
        "refreshInterval": 15
    });

    let pretty = serde_json::to_string_pretty(&root).expect("serialization cannot fail");
    Ok((
        pretty,
        Apply {
            current_was_ours,
            previous,
            installed_command: command,
        },
    ))
}

/// Remove our managed `statusLine`, restoring whatever the user had before.
///
/// Only acts when the current root `statusLine.command` still equals
/// `recorded.installed_command` (i.e. the setting is still ours). In that case it
/// restores `recorded.previous` if present, or deletes the `statusLine` key
/// entirely when there was none. If the user changed `statusLine` after install
/// it is left untouched (no error).
///
/// Returns `Ok(pretty_json)`, or a parse error when `existing` is invalid JSON.
pub fn remove(existing: &str, recorded: &ClaudeStatuslineState) -> serde_json::Result<String> {
    let mut root: Value = serde_json::from_str(existing)?;

    let current_is_ours = root
        .get("statusLine")
        .and_then(|sl| sl.get("command"))
        .and_then(Value::as_str)
        == Some(recorded.installed_command.as_str());

    if current_is_ours {
        match &recorded.previous {
            Some(v) => {
                root["statusLine"] = v.clone();
            }
            None => {
                if let Some(obj) = root.as_object_mut() {
                    obj.remove("statusLine");
                }
            }
        }
    }

    Ok(serde_json::to_string_pretty(&root).expect("serialization cannot fail"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "C:\\p\\ai-handoff.exe";

    #[test]
    fn apply_with_no_statusline_sets_ours_and_previous_none() {
        let (json_str, a) = apply(Some(r#"{"model":"opus"}"#), EXE).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        // unrelated keys preserved
        assert_eq!(v["model"], "opus");
        // our statusLine set with exact shape
        assert_eq!(v["statusLine"]["type"], "command");
        assert_eq!(v["statusLine"]["command"], installed_command(EXE));
        assert_eq!(v["statusLine"]["refreshInterval"], 15);
        // outcome
        assert!(!a.current_was_ours);
        assert!(a.previous.is_none());
        assert_eq!(a.installed_command, installed_command(EXE));
    }

    #[test]
    fn apply_with_none_existing_starts_from_blank_object() {
        let (json_str, a) = apply(None, EXE).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["statusLine"]["command"], installed_command(EXE));
        assert_eq!(v["statusLine"]["refreshInterval"], 15);
        assert!(!a.current_was_ours);
        assert!(a.previous.is_none());
    }

    #[test]
    fn apply_over_foreign_statusline_records_it_as_previous() {
        let foreign = r#"{"statusLine":{"type":"command","command":"my-prompt --fancy"}}"#;
        let (json_str, a) = apply(Some(foreign), EXE).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        // ours is now installed
        assert_eq!(v["statusLine"]["command"], installed_command(EXE));
        assert_eq!(v["statusLine"]["refreshInterval"], 15);
        // foreign captured for restore
        assert!(!a.current_was_ours);
        let prev = a.previous.expect("foreign previous should be recorded");
        assert_eq!(prev["command"], "my-prompt --fancy");
    }

    #[test]
    fn apply_over_our_own_statusline_is_idempotent_with_previous_none() {
        let (first, _) = apply(None, EXE).unwrap();
        let (second, a) = apply(Some(&first), EXE).unwrap();
        let v: Value = serde_json::from_str(&second).unwrap();
        assert_eq!(v["statusLine"]["command"], installed_command(EXE));
        // re-apply over our own setting: recognized as ours, no new previous
        assert!(a.current_was_ours);
        assert!(a.previous.is_none());
    }

    #[test]
    fn remove_restores_recorded_foreign_previous() {
        // start from a doc that currently has OUR statusLine installed
        let (installed, _) = apply(Some(r#"{"model":"opus"}"#), EXE).unwrap();
        let recorded = ClaudeStatuslineState {
            previous: Some(json!({"type":"command","command":"my-prompt --fancy"})),
            installed_command: installed_command(EXE),
        };
        let cleaned = remove(&installed, &recorded).unwrap();
        let v: Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(v["statusLine"]["command"], "my-prompt --fancy");
        // unrelated keys still preserved
        assert_eq!(v["model"], "opus");
    }

    #[test]
    fn remove_deletes_key_when_previous_none() {
        let (installed, _) = apply(Some(r#"{"model":"opus"}"#), EXE).unwrap();
        let recorded = ClaudeStatuslineState {
            previous: None,
            installed_command: installed_command(EXE),
        };
        let cleaned = remove(&installed, &recorded).unwrap();
        let v: Value = serde_json::from_str(&cleaned).unwrap();
        assert!(v.get("statusLine").is_none());
        assert_eq!(v["model"], "opus");
    }

    #[test]
    fn remove_is_noop_when_user_changed_statusline_after_install() {
        // The live doc has a user-chosen statusLine, NOT ours.
        let live = r#"{"statusLine":{"type":"command","command":"user-changed-it"}}"#;
        let recorded = ClaudeStatuslineState {
            previous: Some(json!({"type":"command","command":"old-foreign"})),
            installed_command: installed_command(EXE),
        };
        let cleaned = remove(live, &recorded).unwrap();
        let v: Value = serde_json::from_str(&cleaned).unwrap();
        // untouched: neither restored to previous nor deleted
        assert_eq!(v["statusLine"]["command"], "user-changed-it");
    }

    #[test]
    fn apply_and_remove_return_err_on_invalid_json() {
        assert!(apply(Some("not valid json"), EXE).is_err());
        let recorded = ClaudeStatuslineState {
            previous: None,
            installed_command: installed_command(EXE),
        };
        assert!(remove("not valid json", &recorded).is_err());
    }
}
