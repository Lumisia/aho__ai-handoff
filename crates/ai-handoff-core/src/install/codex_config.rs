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
) -> Result<ConfigEdit, toml_edit::TomlError> {
    let mut doc: DocumentMut = match existing {
        Some(s) => s.parse::<DocumentMut>()?,
        None => DocumentMut::new(),
    };

    // --- [sandbox_workspace_write].writable_roots ---
    let created_sandbox_table = !doc.contains_key("sandbox_workspace_write");
    if created_sandbox_table {
        doc["sandbox_workspace_write"] = Item::Table(Table::new());
    }
    let sandbox = doc["sandbox_workspace_write"]
        .as_table_mut()
        .expect("sandbox_workspace_write is a table");
    let roots = sandbox
        .entry("writable_roots")
        .or_insert(value(Array::new()))
        .as_array_mut()
        .expect("writable_roots is an array");

    let already_present = roots.iter().any(|v| v.as_str() == Some(ipc_dir));
    let writable_root_added = if already_present {
        None
    } else {
        roots.push(ipc_dir);
        Some(ipc_dir.to_string())
    };

    // --- [shell_environment_policy].set.AI_HANDOFF_HOME ---
    let created_env_table = !doc.contains_key("shell_environment_policy");
    if created_env_table {
        doc["shell_environment_policy"] = Item::Table(Table::new());
    }
    let env_table = doc["shell_environment_policy"]
        .as_table_mut()
        .expect("shell_environment_policy is a table");
    let set = env_table
        .entry("set")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .expect("set is a table");

    let env_key_added = if set.contains_key(ENV_KEY) {
        None
    } else {
        set.insert(ENV_KEY, value(ai_home));
        Some(ENV_KEY.to_string())
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
pub fn remove(existing: &str, st: &CodexState) -> Result<String, toml_edit::TomlError> {
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
        assert!(e2.writable_root_added.is_none());
        assert!(e2.env_key_added.is_none());
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
}
