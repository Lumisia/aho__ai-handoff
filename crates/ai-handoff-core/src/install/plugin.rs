//! Plugin bundle GENERATOR.
//!
//! Produces an installed plugin bundle for a given agent into a target
//! directory, embedding the native-binary absolute path into the bundle's
//! `hooks/hooks.json`. The CLI ships self-contained: the bundle's static
//! content (the agent manifest + the 7 skills) is EMBEDDED into the binary at
//! compile time via [`include_str!`] (no extra crate dependency), and only the
//! hooks file is generated at install time with the resolved exe path.
//!
//! This module just produces a tested [`generate_bundle`] that writes to ANY
//! target dir plus the [`PluginRecord`] install-state. Wiring it into the live
//! install paths (and marketplace registration) is a later task.

use std::path::Path;

use crate::capsule::AgentKind;

use super::state::PluginRecord;

// ---------------------------------------------------------------------------
// Embedded static bundle (compile-time)
// ---------------------------------------------------------------------------

/// Embedded Claude plugin manifest, written verbatim to
/// `<root>/.claude-plugin/plugin.json`.
const CLAUDE_MANIFEST: &str = include_str!("../../../../.claude-plugin/plugin.json");

/// Embedded Codex plugin manifest, written verbatim to
/// `<root>/.codex-plugin/plugin.json`.
const CODEX_MANIFEST: &str = include_str!("../../../../.codex-plugin/plugin.json");

/// The 7 skills shipped in every bundle: `(name, SKILL.md contents)`.
const SKILLS: &[(&str, &str)] = &[
    (
        "handoff",
        include_str!("../../../../skills/handoff/SKILL.md"),
    ),
    (
        "handoff-checkpoint",
        include_str!("../../../../skills/handoff-checkpoint/SKILL.md"),
    ),
    (
        "handoff-clear",
        include_str!("../../../../skills/handoff-clear/SKILL.md"),
    ),
    (
        "handoff-config",
        include_str!("../../../../skills/handoff-config/SKILL.md"),
    ),
    (
        "handoff-doctor",
        include_str!("../../../../skills/handoff-doctor/SKILL.md"),
    ),
    (
        "handoff-ratelimit",
        include_str!("../../../../skills/handoff-ratelimit/SKILL.md"),
    ),
    (
        "handoff-recent",
        include_str!("../../../../skills/handoff-recent/SKILL.md"),
    ),
];

// ---------------------------------------------------------------------------
// hooks/hooks.json generation
// ---------------------------------------------------------------------------

/// Build the bundle's `hooks/hooks.json` text for `agent`, embedding the
/// absolute `exe` path into every managed hook command.
///
/// Claude uses the exec form (mirroring [`super::claude`]): `command` is the
/// bare exe with `args` + `_aiHandoff:true` + `timeout:10`, and PostToolUse
/// carries `"matcher":"Write|Edit|Bash"`.
///
/// Codex uses the command-string form (mirroring [`super::codex_hooks`]):
/// `command = managed_command(exe, event_arg)`, `timeout:10`, every event
/// outer entry carries `"matcher":"*"`, and PostToolUse additionally carries a
/// `"statusMessage"`.
fn build_hooks_json(agent: AgentKind, exe: &str) -> String {
    use serde_json::{json, Map, Value};

    let mut hooks = Map::new();

    match agent {
        AgentKind::ClaudeCode => {
            for (event, event_arg) in super::claude::EVENTS.iter().zip(CLAUDE_EVENT_ARGS.iter()) {
                let inner = json!({
                    "type": "command",
                    "command": exe,
                    "args": ["hook", *event_arg, "--agent", "claude-code"],
                    "_aiHandoff": true,
                    "timeout": 10
                });
                let outer = if *event == "PostToolUse" {
                    json!({ "matcher": "Write|Edit|Bash", "hooks": [inner] })
                } else {
                    json!({ "hooks": [inner] })
                };
                hooks.insert(event.to_string(), Value::Array(vec![outer]));
            }
        }
        AgentKind::Codex => {
            for (event, event_arg) in super::codex_hooks::EVENTS.iter().zip(CODEX_EVENT_ARGS.iter())
            {
                let command = super::codex_hooks::managed_command(exe, event_arg);
                let inner = if *event == "PostToolUse" {
                    json!({
                        "type": "command",
                        "command": command,
                        "timeout": 10,
                        "statusMessage": "Checking handoff threshold"
                    })
                } else {
                    json!({
                        "type": "command",
                        "command": command,
                        "timeout": 10
                    })
                };
                let outer = json!({ "matcher": "*", "hooks": [inner] });
                hooks.insert(event.to_string(), Value::Array(vec![outer]));
            }
        }
    }

    let root = json!({ "hooks": Value::Object(hooks) });
    serde_json::to_string_pretty(&root).expect("serialization cannot fail")
}

/// Kebab CLI arg strings for the Claude events (same order as `claude::EVENTS`).
const CLAUDE_EVENT_ARGS: [&str; 4] = ["session-start", "user-prompt", "post-tool-use", "stop"];

/// Kebab CLI arg strings for the Codex events (same order as `codex_hooks::EVENTS`).
const CODEX_EVENT_ARGS: [&str; 4] = ["session-start", "user-prompt", "post-tool-use", "stop"];

// ---------------------------------------------------------------------------
// Atomic write helper (local, to avoid widening mod.rs visibility)
// ---------------------------------------------------------------------------

/// Write `contents` to `path` via a temp file + rename, creating parents.
fn write_text_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ai-handoff".to_string());
    let tmp = path.with_file_name(format!("{file_name}.ai-handoff.tmp"));
    std::fs::write(&tmp, contents)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(first) if path.exists() => {
            std::fs::remove_file(path)?;
            std::fs::rename(&tmp, path).map_err(|second| {
                let _ = std::fs::remove_file(&tmp);
                if second.kind() == std::io::ErrorKind::Other {
                    first
                } else {
                    second
                }
            })
        }
        Err(err) => {
            let _ = std::fs::remove_file(&tmp);
            Err(err)
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Generate an installed plugin bundle for `agent` under `target_root`,
/// embedding the absolute `exe` path into the generated `hooks/hooks.json`.
///
/// Writes (each atomically):
/// - the agent manifest: Claude → `.claude-plugin/plugin.json`, Codex →
///   `.codex-plugin/plugin.json` (embedded content, verbatim),
/// - each of the 7 skills to `skills/<name>/SKILL.md`,
/// - `hooks/hooks.json` with the 4 lifecycle events.
///
/// Idempotent: re-running into the same dir overwrites files cleanly. Returns a
/// [`PluginRecord`] listing the bundle `root` plus the relative paths written
/// (for surgical uninstall).
pub fn generate_bundle(
    agent: AgentKind,
    exe: &str,
    target_root: &Path,
) -> std::io::Result<PluginRecord> {
    std::fs::create_dir_all(target_root)?;

    let mut files: Vec<String> = Vec::new();

    // Agent manifest.
    let (manifest_rel, manifest_body) = match agent {
        AgentKind::ClaudeCode => (".claude-plugin/plugin.json", CLAUDE_MANIFEST),
        AgentKind::Codex => (".codex-plugin/plugin.json", CODEX_MANIFEST),
    };
    write_text_atomic(&target_root.join(manifest_rel), manifest_body)?;
    files.push(manifest_rel.to_string());

    // Skills.
    for (name, body) in SKILLS {
        let rel = format!("skills/{name}/SKILL.md");
        write_text_atomic(&target_root.join(&rel), body)?;
        files.push(rel);
    }

    // Generated hooks with the embedded absolute exe path.
    let hooks_rel = "hooks/hooks.json";
    write_text_atomic(&target_root.join(hooks_rel), &build_hooks_json(agent, exe))?;
    files.push(hooks_rel.to_string());

    Ok(PluginRecord {
        root: target_root.to_string_lossy().into_owned(),
        files,
        marketplace_file: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn read(root: &Path, rel: &str) -> String {
        std::fs::read_to_string(root.join(rel)).unwrap()
    }

    #[test]
    fn generate_claude_bundle_writes_manifest_skills_and_exec_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exe = "C:\\Program Files\\ai-handoff\\ai-handoff.exe";

        let rec = generate_bundle(AgentKind::ClaudeCode, exe, root).unwrap();

        // Manifest exists and equals the embedded content.
        assert_eq!(read(root, ".claude-plugin/plugin.json"), CLAUDE_MANIFEST);

        // All 7 skills exist.
        for (name, body) in SKILLS {
            assert_eq!(read(root, &format!("skills/{name}/SKILL.md")), *body);
        }

        // hooks/hooks.json parses with all 4 events.
        let hooks: Value = serde_json::from_str(&read(root, "hooks/hooks.json")).unwrap();
        for ev in super::super::claude::EVENTS {
            assert!(
                hooks["hooks"][ev].is_array(),
                "missing claude event {ev}"
            );
        }

        // Stop hook uses exec form with the abs exe + _aiHandoff.
        let stop = &hooks["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(stop["command"], exe);
        assert_eq!(stop["args"][0], "hook");
        assert_eq!(stop["args"][1], "stop");
        assert_eq!(stop["args"][3], "claude-code");
        assert_eq!(stop["_aiHandoff"], true);
        assert_eq!(stop["timeout"], 10);

        // PostToolUse outer entry carries the Claude matcher.
        assert_eq!(
            hooks["hooks"]["PostToolUse"][0]["matcher"],
            "Write|Edit|Bash"
        );
        // Non-PostToolUse events have no matcher.
        assert!(hooks["hooks"]["Stop"][0].get("matcher").is_none());

        // Record lists the written relative paths.
        assert_eq!(
            rec.root,
            root.to_string_lossy().into_owned()
        );
        assert!(rec.files.contains(&".claude-plugin/plugin.json".to_string()));
        assert!(rec.files.contains(&"hooks/hooks.json".to_string()));
        assert!(rec
            .files
            .contains(&"skills/handoff/SKILL.md".to_string()));
        assert_eq!(rec.files.len(), 1 + SKILLS.len() + 1);
        assert!(rec.marketplace_file.is_none());
    }

    #[test]
    fn generate_codex_bundle_writes_manifest_and_command_string_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exe = "C:\\Program Files\\ai-handoff\\ai-handoff.exe";

        generate_bundle(AgentKind::Codex, exe, root).unwrap();

        // Codex manifest exists and equals the embedded content.
        assert_eq!(read(root, ".codex-plugin/plugin.json"), CODEX_MANIFEST);
        // Claude manifest must NOT be written for a Codex bundle.
        assert!(!root.join(".claude-plugin/plugin.json").exists());

        let hooks: Value = serde_json::from_str(&read(root, "hooks/hooks.json")).unwrap();
        for ev in super::super::codex_hooks::EVENTS {
            assert!(hooks["hooks"][ev].is_array(), "missing codex event {ev}");
        }

        // Stop command is the managed command string with the abs exe.
        assert_eq!(
            hooks["hooks"]["Stop"][0]["hooks"][0]["command"],
            format!("\"{exe}\" hook stop --agent codex")
        );
        assert_eq!(hooks["hooks"]["Stop"][0]["hooks"][0]["timeout"], 10);

        // PostToolUse matcher is "*" and carries a statusMessage.
        assert_eq!(hooks["hooks"]["PostToolUse"][0]["matcher"], "*");
        assert_eq!(
            hooks["hooks"]["PostToolUse"][0]["hooks"][0]["statusMessage"],
            "Checking handoff threshold"
        );
    }

    #[test]
    fn codex_command_quotes_exe_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exe = "C:\\Program Files\\ai handoff\\ai-handoff.exe";

        generate_bundle(AgentKind::Codex, exe, root).unwrap();
        let hooks: Value = serde_json::from_str(&read(root, "hooks/hooks.json")).unwrap();
        // The whole exe path (spaces and all) is wrapped in one pair of quotes.
        assert_eq!(
            hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            format!("\"{exe}\" hook session-start --agent codex")
        );
    }

    #[test]
    fn generate_bundle_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exe = "C:\\p\\ai-handoff.exe";

        let first = generate_bundle(AgentKind::ClaudeCode, exe, root).unwrap();
        let second = generate_bundle(AgentKind::ClaudeCode, exe, root).unwrap();
        assert_eq!(first, second);
        // Files are present and well-formed after the second run.
        let hooks: Value = serde_json::from_str(&read(root, "hooks/hooks.json")).unwrap();
        assert_eq!(hooks["hooks"]["Stop"][0]["hooks"][0]["command"], exe);
        // No leftover temp files.
        assert!(!root.join("hooks/hooks.json.ai-handoff.tmp").exists());
    }
}
