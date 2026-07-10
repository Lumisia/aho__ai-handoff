//! Plugin bundle GENERATOR.
//!
//! Produces an installed plugin bundle for a given agent into a target
//! directory, embedding the native-binary absolute path into the bundle's
//! `hooks/hooks.json`. The CLI ships self-contained: the bundle's static
//! content (the agent manifest + bundled skills) is EMBEDDED into the binary at
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

/// The skills shipped in every bundle: `(name, SKILL.md contents)`.
const SKILLS: &[(&str, &str)] = &[
    (
        "handoff",
        include_str!("../../../../skills/handoff/SKILL.md"),
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
        "handoff-checkpoint",
        include_str!("../../../../skills/handoff-checkpoint/SKILL.md"),
    ),
];

const LEGACY_USER_SKILLS: &[&str] = &[
    "handoff",
    "handoff-config",
    "handoff-doctor",
    "handoff-checkpoint",
];

// ---------------------------------------------------------------------------
// hooks/hooks.json generation
// ---------------------------------------------------------------------------

/// Build the Codex bundle's `hooks/hooks.json` text, embedding the absolute
/// `exe` path into every managed hook command.
///
/// Codex uses the command-string form (mirroring [`super::codex_hooks`]):
/// `command = managed_command(exe, event_arg)`, `_aiHandoff:true`, `timeout:10`,
/// every event outer entry carries `"matcher":"*"`, and PostToolUse additionally
/// carries a `"statusMessage"`. The `_aiHandoff` flag keeps parity with the
/// direct `codex_hooks` path so `codex_hooks::remove` can key off it.
///
/// Claude has no bundle hooks: Claude only loads hooks from settings.json
/// ([`super::claude`]), never from the `~/.claude/skills` bundle.
fn build_hooks_json(agent: AgentKind, exe: &str) -> String {
    use serde_json::{json, Map, Value};

    debug_assert_eq!(agent, AgentKind::Codex, "only Codex bundles carry hooks");

    let mut hooks = Map::new();
    for (event, event_arg) in super::codex_hooks::EVENTS
        .iter()
        .zip(CODEX_EVENT_ARGS.iter())
    {
        let command = super::codex_hooks::managed_command(exe, event_arg);
        let inner = if *event == "PostToolUse" {
            json!({
                "type": "command",
                "command": command,
                "_aiHandoff": true,
                "timeout": 10,
                "statusMessage": "Checking handoff threshold"
            })
        } else {
            json!({
                "type": "command",
                "command": command,
                "_aiHandoff": true,
                "timeout": 10
            })
        };
        let outer = json!({ "matcher": "*", "hooks": [inner] });
        hooks.insert(event.to_string(), Value::Array(vec![outer]));
    }

    let root = json!({ "hooks": Value::Object(hooks) });
    serde_json::to_string_pretty(&root).expect("serialization cannot fail")
}

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

/// Generate an installed plugin bundle for `agent` under `target_root`.
///
/// Writes (each atomically):
/// - the agent manifest: Claude → `.claude-plugin/plugin.json`, Codex →
///   `.codex-plugin/plugin.json` (embedded content, verbatim),
/// - each bundled skill to `skills/<name>/SKILL.md`,
/// - Codex only: `hooks/hooks.json` with the 4 lifecycle events (embedding the
///   absolute `exe` path). Claude gets NO hooks file — Claude never loads
///   hooks from `~/.claude/skills`, so Claude hooks live in settings.json
///   (see [`super::claude`]); shipping a dead hooks file here only misleads.
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
        AgentKind::ClaudeCode => (".claude-plugin/plugin.json", CLAUDE_MANIFEST.to_string()),
        AgentKind::Codex => {
            let mut manifest: serde_json::Value = serde_json::from_str(CODEX_MANIFEST)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            manifest["hooks"] = serde_json::json!("./hooks/hooks.json");
            (
                ".codex-plugin/plugin.json",
                serde_json::to_string_pretty(&manifest).expect("serialization cannot fail"),
            )
        }
    };
    write_text_atomic(&target_root.join(manifest_rel), &manifest_body)?;
    files.push(manifest_rel.to_string());

    // Skills. The plugin bundle root is fully owned by ai-handoff, so clear
    // stale skill dirs from earlier versions before writing the current set.
    let skills_root = target_root.join("skills");
    if skills_root.exists() {
        std::fs::remove_dir_all(&skills_root)?;
    }
    for (name, body) in SKILLS {
        let rel = format!("skills/{name}/SKILL.md");
        write_text_atomic(&target_root.join(&rel), body)?;
        files.push(rel);
    }

    match agent {
        AgentKind::Codex => {
            // Generated hooks with the embedded absolute exe path.
            let hooks_rel = "hooks/hooks.json";
            write_text_atomic(&target_root.join(hooks_rel), &build_hooks_json(agent, exe))?;
            files.push(hooks_rel.to_string());
        }
        AgentKind::ClaudeCode => {
            // Remove the dead hooks file older installs shipped (never loaded).
            let stale_hooks = target_root.join("hooks");
            if stale_hooks.exists() {
                std::fs::remove_dir_all(&stale_hooks)?;
            }
        }
    }

    Ok(PluginRecord {
        root: target_root.to_string_lossy().into_owned(),
        files,
        marketplace_file: None,
    })
}

/// Remove old plain user skills created by previous ai-handoff plugin-mode
/// installs. Current plugin mode exposes skills only through the plugin bundle,
/// which keeps the UI list namespaced as `ai-handoff:<skill>`.
pub fn remove_handoff_user_skills(skills_root: &Path) -> std::io::Result<()> {
    remove_legacy_user_skills(skills_root)
}

fn remove_legacy_user_skills(skills_root: &Path) -> std::io::Result<()> {
    for name in LEGACY_USER_SKILLS {
        let dir = skills_root.join(name);
        let Some(existing) = read_existing_skill(&dir)? else {
            continue;
        };
        if existing.contains("ai-handoff") {
            std::fs::remove_dir_all(dir)?;
        }
    }
    Ok(())
}

fn read_existing_skill(target_root: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(target_root.join("SKILL.md")) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

// ---------------------------------------------------------------------------
// Codex personal marketplace registration (~/.agents/plugins/marketplace.json)
// ---------------------------------------------------------------------------

/// The plugin name we register in the personal marketplace.
const MARKETPLACE_PLUGIN_NAME: &str = "ai-handoff";

/// The marketplace `name` Codex pairs with the plugin name to form the enable
/// key (`<plugin>@<marketplace>`). Kept in sync with
/// [`super::codex_config::PLUGIN_ENABLE_KEY`].
const MARKETPLACE_NAME: &str = "claude-codex-auto-handoff";
const MARKETPLACE_PLUGIN_PATH: &str = "./.agents/plugins/ai-handoff";

/// Error from the personal-marketplace merge/remove helpers.
#[derive(Debug, thiserror::Error)]
pub enum MarketplaceError {
    #[error("marketplace.json parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("marketplace.json has an unexpected shape: {0}")]
    UnexpectedShape(&'static str),
}

/// Merge our `ai-handoff` entry into the personal `marketplace.json` text.
///
/// Parses `existing` (or seeds a fresh `{name, plugins:[]}` document when
/// `None`), then upserts our local-source plugin entry without duplicating it.
/// Foreign entries are preserved. Propagates a parse error so the caller aborts
/// rather than clobbering a malformed file.
///
/// Returns the pretty-printed JSON text to write.
pub fn merge_marketplace_entry(existing: Option<&str>) -> Result<String, MarketplaceError> {
    use serde_json::{json, Value};

    let mut root: Value = match existing {
        Some(s) => serde_json::from_str::<Value>(s)?,
        None => json!({
            "name": MARKETPLACE_NAME,
            "description": "Automatic, integrity-checked handoff and verified memory recall between Claude Code and Codex.",
            "plugins": []
        }),
    };

    if !root.is_object() {
        return Err(MarketplaceError::UnexpectedShape(
            "marketplace.json root is not an object",
        ));
    }
    // A name is required for Codex to derive the `<plugin>@<marketplace>` key.
    if root.get("name").and_then(Value::as_str).is_none() {
        root["name"] = json!(MARKETPLACE_NAME);
    }

    // `plugins` must be an array; create it when missing, error on wrong type.
    let plugins_val = root
        .as_object_mut()
        .unwrap()
        .entry("plugins")
        .or_insert_with(|| json!([]));
    let plugins = plugins_val
        .as_array_mut()
        .ok_or(MarketplaceError::UnexpectedShape("plugins is not an array"))?;

    let plugin_entry = json!({
        "name": MARKETPLACE_PLUGIN_NAME,
        "source": { "source": "local", "path": MARKETPLACE_PLUGIN_PATH },
        "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
        "category": "Developer Tools"
    });
    let existing_index = plugins
        .iter()
        .position(|p| p.get("name").and_then(Value::as_str) == Some(MARKETPLACE_PLUGIN_NAME));
    if let Some(index) = existing_index {
        plugins[index] = plugin_entry;
    } else {
        plugins.push(plugin_entry);
    }

    Ok(serde_json::to_string_pretty(&root).expect("serialization cannot fail"))
}

/// Remove our `ai-handoff` entry from the personal `marketplace.json` text,
/// preserving every foreign entry. An empty `plugins` array afterwards is left
/// in place (harmless). Propagates parse errors rather than clobbering.
pub fn remove_marketplace_entry(existing: &str) -> Result<String, MarketplaceError> {
    use serde_json::Value;

    let mut root: Value = serde_json::from_str(existing)?;

    if let Some(plugins) = root.get_mut("plugins").and_then(Value::as_array_mut) {
        plugins.retain(|p| p.get("name").and_then(Value::as_str) != Some(MARKETPLACE_PLUGIN_NAME));
    }

    Ok(serde_json::to_string_pretty(&root).expect("serialization cannot fail"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn read(root: &Path, rel: &str) -> String {
        std::fs::read_to_string(root.join(rel)).unwrap()
    }

    #[test]
    fn generate_claude_bundle_writes_manifest_and_skills_without_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exe = "C:\\Program Files\\ai-handoff\\ai-handoff.exe";

        let rec = generate_bundle(AgentKind::ClaudeCode, exe, root).unwrap();

        // Manifest exists and equals the embedded content.
        assert_eq!(read(root, ".claude-plugin/plugin.json"), CLAUDE_MANIFEST);

        // All bundled skills exist.
        for (name, body) in SKILLS {
            assert_eq!(read(root, &format!("skills/{name}/SKILL.md")), *body);
        }
        let skill_names: Vec<&str> = SKILLS.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            skill_names,
            [
                "handoff",
                "handoff-config",
                "handoff-doctor",
                "handoff-checkpoint"
            ]
        );

        // NO hooks file: Claude never loads hooks from `~/.claude/skills`, so
        // Claude hooks belong in settings.json (super::claude::apply).
        assert!(!root.join("hooks").exists());

        // Record lists the written relative paths.
        assert_eq!(rec.root, root.to_string_lossy().into_owned());
        assert!(rec
            .files
            .contains(&".claude-plugin/plugin.json".to_string()));
        assert!(!rec.files.contains(&"hooks/hooks.json".to_string()));
        assert!(rec.files.contains(&"skills/handoff/SKILL.md".to_string()));
        assert!(rec
            .files
            .contains(&"skills/handoff-checkpoint/SKILL.md".to_string()));
        assert!(rec
            .files
            .contains(&"skills/handoff-doctor/SKILL.md".to_string()));
        assert!(rec
            .files
            .contains(&"skills/handoff-config/SKILL.md".to_string()));
        assert_eq!(rec.files.len(), 1 + SKILLS.len());
        assert!(rec.marketplace_file.is_none());
    }

    #[test]
    fn generate_claude_bundle_removes_stale_hooks_from_older_install() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("hooks")).unwrap();
        std::fs::write(root.join("hooks/hooks.json"), "{}").unwrap();

        generate_bundle(AgentKind::ClaudeCode, "C:\\p\\ai-handoff.exe", root).unwrap();

        assert!(!root.join("hooks").exists());
    }

    #[test]
    fn remove_handoff_user_skills_removes_only_managed_plain_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("skills");
        std::fs::create_dir_all(root.join("handoff-config")).unwrap();
        std::fs::create_dir_all(root.join("handoff-doctor")).unwrap();
        std::fs::create_dir_all(root.join("other-skill")).unwrap();
        std::fs::write(
            root.join("handoff-config/SKILL.md"),
            include_str!("../../../../skills/handoff-config/SKILL.md"),
        )
        .unwrap();
        std::fs::write(
            root.join("handoff-doctor/SKILL.md"),
            "---\nname: handoff-doctor\ndescription: user skill\n---\nforeign\n",
        )
        .unwrap();
        std::fs::write(
            root.join("other-skill/SKILL.md"),
            "---\nname: other-skill\ndescription: user skill\n---\nforeign\n",
        )
        .unwrap();

        remove_handoff_user_skills(&root).unwrap();

        assert!(!root.join("handoff-config").exists());
        assert!(root.join("handoff-doctor/SKILL.md").exists());
        assert!(root.join("other-skill/SKILL.md").exists());
    }

    #[test]
    fn generate_codex_bundle_writes_manifest_and_command_string_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exe = "C:\\Program Files\\ai-handoff\\ai-handoff.exe";

        generate_bundle(AgentKind::Codex, exe, root).unwrap();

        // Codex manifest is based on the embedded source manifest, but installed
        // bundles point at the generated native-exe hook file.
        let source_manifest: Value = serde_json::from_str(CODEX_MANIFEST).unwrap();
        let installed_manifest: Value =
            serde_json::from_str(&read(root, ".codex-plugin/plugin.json")).unwrap();
        assert_eq!(installed_manifest["name"], source_manifest["name"]);
        assert_eq!(installed_manifest["version"], source_manifest["version"]);
        assert_eq!(installed_manifest["hooks"], "./hooks/hooks.json");
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

        // Every Codex inner hook carries `_aiHandoff: true` for parity with the
        // direct `codex_hooks` path — `codex_hooks::remove` keys off this flag.
        for ev in super::super::codex_hooks::EVENTS {
            assert_eq!(
                hooks["hooks"][ev][0]["hooks"][0]["_aiHandoff"].as_bool(),
                Some(true),
                "codex bundle event {ev} missing _aiHandoff:true"
            );
        }

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

        let first = generate_bundle(AgentKind::Codex, exe, root).unwrap();
        let second = generate_bundle(AgentKind::Codex, exe, root).unwrap();
        assert_eq!(first, second);
        // Files are present and well-formed after the second run.
        let hooks: Value = serde_json::from_str(&read(root, "hooks/hooks.json")).unwrap();
        assert_eq!(
            hooks["hooks"]["Stop"][0]["hooks"][0]["command"],
            format!("\"{exe}\" hook stop --agent codex")
        );
        // No leftover temp files.
        assert!(!root.join("hooks/hooks.json.ai-handoff.tmp").exists());
    }

    #[test]
    fn merge_marketplace_seeds_fresh_document() {
        let out = merge_marketplace_entry(None).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["name"], MARKETPLACE_NAME);
        let plugins = v["plugins"].as_array().unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0]["name"], MARKETPLACE_PLUGIN_NAME);
        assert_eq!(plugins[0]["source"]["source"], "local");
        assert_eq!(plugins[0]["source"]["path"], MARKETPLACE_PLUGIN_PATH);
        assert_eq!(plugins[0]["policy"]["installation"], "AVAILABLE");
        assert_eq!(plugins[0]["policy"]["authentication"], "ON_INSTALL");
        assert_eq!(plugins[0]["category"], "Developer Tools");
    }

    #[test]
    fn merge_marketplace_preserves_foreign_and_is_idempotent() {
        let src = r#"{"name":"mine","plugins":[{"name":"other","source":{"source":"url","url":"https://x"}}]}"#;
        let once = merge_marketplace_entry(Some(src)).unwrap();
        let twice = merge_marketplace_entry(Some(&once)).unwrap();
        let v: Value = serde_json::from_str(&twice).unwrap();
        // foreign marketplace name preserved
        assert_eq!(v["name"], "mine");
        let plugins = v["plugins"].as_array().unwrap();
        // foreign + ours, no duplicate after the second merge
        assert_eq!(plugins.len(), 2);
        assert!(plugins.iter().any(|p| p["name"] == "other"));
        assert_eq!(
            plugins
                .iter()
                .filter(|p| p["name"] == MARKETPLACE_PLUGIN_NAME)
                .count(),
            1
        );
    }

    #[test]
    fn merge_marketplace_updates_existing_ai_handoff_entry() {
        let src = r#"{"plugins":[{"name":"ai-handoff","source":{"source":"local","path":"./ai-handoff"},"category":"Old"}]}"#;
        let out = merge_marketplace_entry(Some(src)).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let plugin = &v["plugins"].as_array().unwrap()[0];
        assert_eq!(plugin["source"]["path"], MARKETPLACE_PLUGIN_PATH);
        assert_eq!(plugin["policy"]["installation"], "AVAILABLE");
        assert_eq!(plugin["category"], "Developer Tools");
    }

    #[test]
    fn merge_marketplace_errors_on_malformed_json() {
        assert!(merge_marketplace_entry(Some("{ not json")).is_err());
    }

    #[test]
    fn merge_marketplace_errors_when_plugins_wrong_type() {
        assert!(merge_marketplace_entry(Some(r#"{"plugins":"x"}"#)).is_err());
    }

    #[test]
    fn remove_marketplace_strips_only_ours() {
        let merged = merge_marketplace_entry(Some(
            r#"{"name":"mine","plugins":[{"name":"other","source":{"source":"url","url":"https://x"}}]}"#,
        ))
        .unwrap();
        let cleaned = remove_marketplace_entry(&merged).unwrap();
        let v: Value = serde_json::from_str(&cleaned).unwrap();
        let plugins = v["plugins"].as_array().unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0]["name"], "other");
    }

    #[test]
    fn remove_marketplace_errors_on_malformed_json() {
        assert!(remove_marketplace_entry("{ not json").is_err());
    }

    #[test]
    fn checkpoint_skill_uses_guidance_for_claude_and_codex_bundles() {
        for agent in [AgentKind::ClaudeCode, AgentKind::Codex] {
            let dir = tempfile::tempdir().unwrap();
            generate_bundle(agent, "C:\\tools\\ai-handoff.exe", dir.path()).unwrap();
            let skill = read(dir.path(), "skills/handoff-checkpoint/SKILL.md");

            assert!(skill.contains("ai-handoff checkpoint guidance --agent <self> --json"));
            assert!(skill.contains("/handoff checkpoint md"));
            assert!(skill.contains("/handoff checkpoint json"));
            for field in [
                "language",
                "limits",
                "input_format",
                "input_template",
                "command",
            ] {
                assert!(skill.contains(field), "missing guidance field {field}");
            }
            assert!(!skill.contains("config get capsule.language"));
        }
    }
}
