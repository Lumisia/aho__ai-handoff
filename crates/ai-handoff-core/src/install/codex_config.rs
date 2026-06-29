//! Surgical edits to the user's `~/.codex/config.toml`.
//!
//! This is the central **never-clobber** module: it adds exactly two things to a
//! large, hand-maintained config and removes only those two on uninstall, while
//! preserving every other table, key, comment, literal string and quoted key
//! byte-for-byte.
//!
//! The two managed additions are:
//! 1. `[sandbox_workspace_write].writable_roots` += our IPC directory.
//! 2. `[shell_environment_policy].set.AI_HANDOFF_HOME` = our AI home directory.
//!
//! See `https://developers.openai.com/codex/config-reference`:
//! `[sandbox_workspace_write]` is a top-level table whose `writable_roots` is an
//! array of strings; `[shell_environment_policy]` is a top-level table whose
//! `set` maps `VAR = "value"` for spawned commands.

use toml_edit::{value, Array, DocumentMut, Item, Table};

use crate::install::state::CodexState;

/// The environment variable we manage under `[shell_environment_policy].set`.
const ENV_KEY: &str = "AI_HANDOFF_HOME";

/// The dotted `[plugins."<KEY>"]` table key Codex reads to enable our bundle.
/// `name@marketplace` per the Codex plugin model (marketplace name is the
/// `name` field of `~/.agents/plugins/marketplace.json`).
pub const PLUGIN_ENABLE_KEY: &str = "ai-handoff@claude-codex-auto-handoff";

/// Error type for config edit operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigEditError {
    #[error("config.toml parse error: {0}")]
    Parse(#[from] toml_edit::TomlError),
    #[error("config.toml has an unexpected shape: {0}")]
    UnexpectedShape(&'static str),
}

/// Outcome of an [`apply`] call: the serialized document plus a record of what we
/// changed, so [`remove`] can later undo exactly those changes and nothing more.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigEdit {
    /// The full serialized `config.toml` text after our edits.
    pub text: String,
    /// The writable root we pushed, or `None` if it was already present.
    pub writable_root_added: Option<String>,
    /// `true` only if `[sandbox_workspace_write]` did not exist before this call.
    pub created_sandbox_table: bool,
    /// The env key we set, or `None` if it was already present.
    pub env_key_added: Option<String>,
    /// `true` only if `[shell_environment_policy]` did not exist before this call.
    pub created_env_table: bool,
}

/// Surgically add our writable root and env var to `existing` (or to an empty
/// document when `existing` is `None`), preserving everything else.
///
/// Parses with [`toml_edit::DocumentMut`] and **propagates any parse error** so
/// the caller can abort rather than ever clobbering an existing config with a
/// blank document.
pub fn apply(
    existing: Option<&str>,
    ipc_dir: &str,
    ai_home: &str,
) -> Result<ConfigEdit, ConfigEditError> {
    let mut doc: DocumentMut = match existing {
        Some(s) => s.parse::<DocumentMut>()?,
        None => DocumentMut::new(),
    };

    // --- [sandbox_workspace_write].writable_roots ---
    let created_sandbox_table = !doc.contains_key("sandbox_workspace_write");
    if created_sandbox_table {
        doc["sandbox_workspace_write"] = Item::Table(Table::new());
    }
    let sandbox =
        doc["sandbox_workspace_write"]
            .as_table_mut()
            .ok_or(ConfigEditError::UnexpectedShape(
                "sandbox_workspace_write is not a table",
            ))?;
    let roots = sandbox
        .entry("writable_roots")
        .or_insert(value(Array::new()))
        .as_array_mut()
        .ok_or(ConfigEditError::UnexpectedShape(
            "writable_roots is not an array",
        ))?;

    if !roots.iter().any(|v| v.as_str() == Some(ipc_dir)) {
        roots.push(ipc_dir);
    }
    // Our IPC dir lives under our own AI home, so its presence is unambiguous
    // ownership. Record it as managed whenever it is present after this apply
    // (not only when THIS call pushed it) — otherwise an idempotent re-install
    // would report `None`, overwrite the install-state, and leave uninstall
    // unable to remove the root an earlier install added.
    let writable_root_added = Some(ipc_dir.to_string());

    // --- [shell_environment_policy].set.AI_HANDOFF_HOME ---
    let created_env_table = !doc.contains_key("shell_environment_policy");
    if created_env_table {
        doc["shell_environment_policy"] = Item::Table(Table::new());
    }
    let env_table =
        doc["shell_environment_policy"]
            .as_table_mut()
            .ok_or(ConfigEditError::UnexpectedShape(
                "shell_environment_policy is not a table",
            ))?;
    let set = env_table
        .entry("set")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or(ConfigEditError::UnexpectedShape(
            "shell_environment_policy.set is not a table",
        ))?;

    if !set.contains_key(ENV_KEY) {
        set.insert(ENV_KEY, value(ai_home));
    }
    // Claim ownership of the env key only when its value is the home we manage,
    // so we never remove a same-named key the user set to a different value.
    // Recorded by presence (not only when THIS call inserted it) so an
    // idempotent re-install keeps ownership for a later uninstall.
    let env_key_added = if set.get(ENV_KEY).and_then(|v| v.as_str()) == Some(ai_home) {
        Some(ENV_KEY.to_string())
    } else {
        None
    };

    Ok(ConfigEdit {
        text: doc.to_string(),
        writable_root_added,
        created_sandbox_table,
        env_key_added,
        created_env_table,
    })
}

/// Surgically remove only our recorded changes from `existing`, leaving every
/// other table/key untouched. Propagates parse errors rather than clobbering.
pub fn remove(existing: &str, st: &CodexState) -> Result<String, ConfigEditError> {
    let mut doc: DocumentMut = existing.parse::<DocumentMut>()?;

    // --- writable_roots ---
    if let Some(root) = st.writable_root_added.as_deref() {
        if let Some(roots) = doc
            .get_mut("sandbox_workspace_write")
            .and_then(|t| t.as_table_mut())
            .and_then(|t| t.get_mut("writable_roots"))
            .and_then(|a| a.as_array_mut())
        {
            roots.retain(|v| v.as_str() != Some(root));
            let now_empty = roots.is_empty();
            if now_empty && st.created_sandbox_table {
                doc.remove("sandbox_workspace_write");
            }
        }
    }

    // --- env set ---
    if let Some(key) = st.env_key_added.as_deref() {
        if let Some(set) = doc
            .get_mut("shell_environment_policy")
            .and_then(|t| t.as_table_mut())
            .and_then(|t| t.get_mut("set"))
            .and_then(|s| s.as_table_mut())
        {
            set.remove(key);
            let now_empty = set.is_empty();
            if now_empty && st.created_env_table {
                doc.remove("shell_environment_policy");
            }
        }
    }

    Ok(doc.to_string())
}

/// Set `[plugins."ai-handoff@claude-codex-auto-handoff"] enabled = true` in
/// `existing` (or an empty document when `None`), preserving every other
/// `[plugins.*]` table and the rest of the config. Never-clobber: parses with
/// [`toml_edit::DocumentMut`] and propagates parse errors so the caller aborts
/// rather than overwriting a real config with a blank one.
pub fn enable_plugin(existing: Option<&str>) -> Result<String, ConfigEditError> {
    let mut doc: DocumentMut = match existing {
        Some(s) => s.parse::<DocumentMut>()?,
        None => DocumentMut::new(),
    };

    let plugins = doc
        .entry("plugins")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or(ConfigEditError::UnexpectedShape("plugins is not a table"))?;
    // Render `[plugins.<key>]` as its own table header rather than inline.
    plugins.set_implicit(true);
    let entry = plugins
        .entry(PLUGIN_ENABLE_KEY)
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or(ConfigEditError::UnexpectedShape(
            "plugins.<ai-handoff> is not a table",
        ))?;
    entry.insert("enabled", value(true));

    Ok(doc.to_string())
}

/// Remove our `[plugins."ai-handoff@..."]` table from `existing`, preserving
/// every other `[plugins.*]` table. If `[plugins]` becomes empty afterwards it
/// is dropped too. Propagates parse errors rather than clobbering.
pub fn disable_plugin(existing: &str) -> Result<String, ConfigEditError> {
    let mut doc: DocumentMut = existing.parse::<DocumentMut>()?;

    if let Some(plugins) = doc.get_mut("plugins").and_then(|t| t.as_table_mut()) {
        plugins.remove(PLUGIN_ENABLE_KEY);
        if plugins.is_empty() {
            doc.remove("plugins");
        }
    }

    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::state::CodexState;

    fn fixture() -> String {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codex-config-complex.toml"
        ))
        .unwrap()
    }

    #[test]
    fn adds_two_tables_and_preserves_everything_else() {
        let src = fixture();
        let e = apply(
            Some(&src),
            "C:\\Users\\PC\\.ai-handoff\\ipc",
            "C:\\Users\\PC\\.ai-handoff",
        )
        .unwrap();
        let doc: toml_edit::DocumentMut = e.text.parse().unwrap();
        // our additions:
        assert!(e.created_sandbox_table);
        assert_eq!(
            e.writable_root_added.as_deref(),
            Some("C:\\Users\\PC\\.ai-handoff\\ipc")
        );
        assert!(doc["sandbox_workspace_write"]["writable_roots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("C:\\Users\\PC\\.ai-handoff\\ipc")));
        assert_eq!(
            doc["shell_environment_policy"]["set"]["AI_HANDOFF_HOME"].as_str(),
            Some("C:\\Users\\PC\\.ai-handoff")
        );
        // preserved (spot-check structurally distinctive bits):
        assert_eq!(doc["sandbox_mode"].as_str(), Some("workspace-write"));
        assert_eq!(doc["windows"]["sandbox"].as_str(), Some("unelevated"));
        assert!(e
            .text
            .contains(r#"[projects.'c:\users\pc\desktop\ai-handoff']"#));
        assert!(e.text.contains(
            r#"command = 'C:\Users\PC\AppData\Local\OpenAI\Codex\runtimes\node_repl.exe'"#
        ));
        assert!(e
            .text
            .contains("ai-handoff@claude-codex-auto-handoff:hooks/hooks-codex.json:session_start"));

        // Strengthened preservation: every line present in the original must
        // still be present in the output (no original line dropped).
        let before_lines: std::collections::HashMap<&str, usize> =
            src.lines()
                .fold(std::collections::HashMap::new(), |mut m, l| {
                    *m.entry(l).or_insert(0) += 1;
                    m
                });
        let after_lines: std::collections::HashMap<&str, usize> =
            e.text
                .lines()
                .fold(std::collections::HashMap::new(), |mut m, l| {
                    *m.entry(l).or_insert(0) += 1;
                    m
                });
        for (line, &count) in &before_lines {
            let after_count = after_lines.get(line).copied().unwrap_or(0);
            assert!(
                after_count >= count,
                "original line dropped: {:?} (expected {} occurrences, found {})",
                line,
                count,
                after_count
            );
        }
    }

    #[test]
    fn apply_is_idempotent() {
        let e1 = apply(Some(&fixture()), "C:/ipc", "C:/home").unwrap();
        let e2 = apply(Some(&e1.text), "C:/ipc", "C:/home").unwrap();
        let doc: toml_edit::DocumentMut = e2.text.parse().unwrap();
        assert_eq!(
            doc["sandbox_workspace_write"]["writable_roots"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(!e2.created_sandbox_table); // already existed the second time
                                            // Managed entries are recorded by presence, so a re-apply still reports
                                            // them (they are not duplicated — the array length stays 1 above) — this
                                            // is what lets a later uninstall remove them after an idempotent install.
        assert_eq!(e2.writable_root_added.as_deref(), Some("C:/ipc"));
        assert_eq!(e2.env_key_added.as_deref(), Some("AI_HANDOFF_HOME"));
    }

    #[test]
    fn apply_does_not_claim_user_env_key_with_different_value() {
        // User already set AI_HANDOFF_HOME to their own value before install.
        let src = "[shell_environment_policy.set]\nAI_HANDOFF_HOME = \"D:\\\\theirs\"\n";
        let e = apply(Some(src), "C:/ipc", "C:/home").unwrap();
        // We must not claim ownership (uninstall would wrongly remove their key).
        assert!(e.env_key_added.is_none());
        // And we must not have overwritten their value.
        let doc: toml_edit::DocumentMut = e.text.parse().unwrap();
        assert_eq!(
            doc["shell_environment_policy"]["set"]["AI_HANDOFF_HOME"].as_str(),
            Some("D:\\theirs")
        );
    }

    #[test]
    fn remove_strips_only_ours_and_keeps_user_added_roots() {
        // After install, the user adds another writable root themselves.
        let e = apply(Some(&fixture()), "C:/ipc", "C:/home").unwrap();
        let mut doc: toml_edit::DocumentMut = e.text.parse().unwrap();
        doc["sandbox_workspace_write"]["writable_roots"]
            .as_array_mut()
            .unwrap()
            .push("C:/user/added/root");
        let after_user = doc.to_string();
        let st = CodexState {
            writable_root_added: Some("C:/ipc".into()),
            created_sandbox_table: true,
            env_key_added: Some("AI_HANDOFF_HOME".into()),
            created_env_table: true,
            ..Default::default()
        };
        let cleaned = remove(&after_user, &st).unwrap();
        let cdoc: toml_edit::DocumentMut = cleaned.parse().unwrap();
        // our root gone, user's root kept, table NOT removed (non-empty)
        let roots = cdoc["sandbox_workspace_write"]["writable_roots"]
            .as_array()
            .unwrap();
        assert!(roots.iter().all(|v| v.as_str() != Some("C:/ipc")));
        assert!(roots
            .iter()
            .any(|v| v.as_str() == Some("C:/user/added/root")));
        // env table created solely by us and now empty -> removed
        assert!(cdoc.get("shell_environment_policy").is_none());
        // unrelated content still present
        assert_eq!(cdoc["sandbox_mode"].as_str(), Some("workspace-write"));
    }

    #[test]
    fn apply_returns_err_on_invalid_toml() {
        assert!(apply(Some("not = = valid toml"), "C:/ipc", "C:/home").is_err());
    }

    #[test]
    fn apply_returns_err_when_writable_roots_is_wrong_type() {
        let bad = "sandbox_workspace_write = { writable_roots = \"x\" }\n";
        let result = apply(Some(bad), "C:/ipc", "C:/home");
        assert!(
            result.is_err(),
            "expected Err when writable_roots is a string, got Ok"
        );
    }

    #[test]
    fn apply_returns_err_when_sandbox_workspace_write_is_wrong_type() {
        let bad = "sandbox_workspace_write = \"x\"\n";
        let result = apply(Some(bad), "C:/ipc", "C:/home");
        assert!(
            result.is_err(),
            "expected Err when sandbox_workspace_write is a string, got Ok"
        );
    }

    #[test]
    fn apply_returns_err_when_shell_environment_policy_is_wrong_type() {
        let bad = "shell_environment_policy = \"x\"\n";
        let result = apply(Some(bad), "C:/ipc", "C:/home");
        assert!(
            result.is_err(),
            "expected Err when shell_environment_policy is a string, got Ok"
        );
    }

    #[test]
    fn apply_returns_err_when_set_is_wrong_type() {
        let bad = "[shell_environment_policy]\nset = \"x\"\n";
        let result = apply(Some(bad), "C:/ipc", "C:/home");
        assert!(
            result.is_err(),
            "expected Err when shell_environment_policy.set is a string, got Ok"
        );
    }

    #[test]
    fn enable_plugin_sets_enabled_true_on_empty_config() {
        let out = enable_plugin(None).unwrap();
        let doc: DocumentMut = out.parse().unwrap();
        assert_eq!(
            doc["plugins"][PLUGIN_ENABLE_KEY]["enabled"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn enable_plugin_preserves_foreign_plugin_and_other_config() {
        let src = concat!(
            "sandbox_mode = \"workspace-write\"\n",
            "[plugins.\"other@x\"]\n",
            "enabled = false\n",
        );
        let out = enable_plugin(Some(src)).unwrap();
        let doc: DocumentMut = out.parse().unwrap();
        // ours added
        assert_eq!(
            doc["plugins"][PLUGIN_ENABLE_KEY]["enabled"].as_bool(),
            Some(true)
        );
        // foreign plugin untouched
        assert_eq!(doc["plugins"]["other@x"]["enabled"].as_bool(), Some(false));
        // unrelated config preserved
        assert_eq!(doc["sandbox_mode"].as_str(), Some("workspace-write"));
    }

    #[test]
    fn enable_plugin_is_idempotent() {
        let first = enable_plugin(None).unwrap();
        let second = enable_plugin(Some(&first)).unwrap();
        let doc: DocumentMut = second.parse().unwrap();
        assert_eq!(
            doc["plugins"][PLUGIN_ENABLE_KEY]["enabled"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn disable_plugin_removes_only_ours_and_keeps_foreign() {
        let enabled = enable_plugin(Some("[plugins.\"other@x\"]\nenabled = true\n")).unwrap();
        let disabled = disable_plugin(&enabled).unwrap();
        let doc: DocumentMut = disabled.parse().unwrap();
        // ours gone
        assert!(doc["plugins"].get(PLUGIN_ENABLE_KEY).is_none());
        // foreign preserved
        assert_eq!(doc["plugins"]["other@x"]["enabled"].as_bool(), Some(true));
    }

    #[test]
    fn disable_plugin_drops_empty_plugins_table() {
        let enabled = enable_plugin(None).unwrap();
        let disabled = disable_plugin(&enabled).unwrap();
        let doc: DocumentMut = disabled.parse().unwrap();
        assert!(doc.get("plugins").is_none());
    }

    #[test]
    fn enable_plugin_returns_err_on_invalid_toml() {
        assert!(enable_plugin(Some("not = = valid")).is_err());
    }
}
