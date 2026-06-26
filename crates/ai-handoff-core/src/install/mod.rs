pub mod backup;
pub mod claude;
pub mod claude_statusline;
pub mod codex_config;
pub mod codex_hooks;
pub mod detect;
pub mod diff;
pub mod duplicate;
pub mod state;

pub use backup::{backup_file, backup_path};
pub use detect::{detect_agents, targets_for, AgentPresence, InstallTargets};
pub use state::{
    load, save, state_path, AutostartKind, AutostartState, ClaudeState, ClaudeStatuslineState,
    CodexState, FileMod, InstallState,
};

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

/// The result of [`plan_install`]: the per-file diffs we would write, any
/// duplicate-hook findings worth warning the user about, and which agents are
/// present. Computed entirely in memory — no disk writes.
pub struct InstallPlan {
    pub file_plans: Vec<diff::FilePlan>,
    pub duplicates: Vec<duplicate::DuplicateFinding>,
    pub agents: AgentPresence,
}

/// Read an existing file's contents, returning `None` when it is absent.
///
/// Any other I/O error (permissions, etc.) is propagated so we abort rather than
/// treat a real failure as "absent" and risk clobbering.
fn read_existing(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Compute the full set of file edits we would make, without touching disk.
///
/// For every present agent we read its existing config (if any), run the
/// matching `apply(...)`, and assemble a [`diff::FilePlan`]. A malformed
/// existing config makes `apply(...)` return `Err`, which we propagate via `?`
/// so the whole plan aborts **before** any write happens — the never-clobber
/// guarantee. We also gather advisory [`duplicate::detect`] findings.
pub fn plan_install(
    t: &InstallTargets,
    agents: &AgentPresence,
    _now: DateTime<Utc>,
) -> anyhow::Result<InstallPlan> {
    let ipc_dir = t.ipc_dir.to_string_lossy().into_owned();
    let ai_home = t.home.to_string_lossy().into_owned();
    let exe = t.exe.to_string_lossy().into_owned();

    let mut file_plans: Vec<diff::FilePlan> = Vec::new();

    let codex_config_existing = if agents.codex {
        read_existing(&t.codex_config)?
    } else {
        None
    };
    let claude_settings_existing = if agents.claude {
        read_existing(&t.claude_settings)?
    } else {
        None
    };

    if agents.codex {
        // Codex hooks.json
        let hooks_existing = read_existing(&t.codex_hooks)?;
        let (hooks_after, _events) = codex_hooks::apply(hooks_existing.as_deref(), &exe)?;
        file_plans.push(diff::FilePlan {
            path: t.codex_hooks.to_string_lossy().into_owned(),
            before: hooks_existing,
            after: hooks_after,
        });

        // Codex config.toml
        let edit = codex_config::apply(codex_config_existing.as_deref(), &ipc_dir, &ai_home)?;
        file_plans.push(diff::FilePlan {
            path: t.codex_config.to_string_lossy().into_owned(),
            before: codex_config_existing.clone(),
            after: edit.text,
        });
    }

    if agents.claude {
        let (settings_after, _events) = claude::apply(claude_settings_existing.as_deref(), &exe)?;
        // Preview the same final document apply_install writes: the hooks edit
        // plus our managed root statusLine key.
        let (settings_after_all, _sl_apply) =
            claude_statusline::apply(Some(&settings_after), &exe)?;
        file_plans.push(diff::FilePlan {
            path: t.claude_settings.to_string_lossy().into_owned(),
            before: claude_settings_existing.clone(),
            after: settings_after_all,
        });
    }

    let duplicates = duplicate::detect(
        codex_config_existing.as_deref(),
        claude_settings_existing.as_deref(),
    );

    Ok(InstallPlan {
        file_plans,
        duplicates,
        agents: *agents,
    })
}

/// Write the file `plan.after` text to `path`, backing up any existing file
/// first and creating parent directories as needed. Returns the backup path (if
/// a file existed) as a string for recording into [`InstallState`].
fn write_with_backup(
    path: &Path,
    after: &str,
    now: DateTime<Utc>,
) -> std::io::Result<Option<String>> {
    let backup = backup::backup_file(path, now)?;
    write_text_atomic(path, after)?;
    Ok(backup.map(|b| b.to_string_lossy().into_owned()))
}

fn atomic_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ai-handoff".to_string());
    path.with_file_name(format!("{file_name}.ai-handoff.tmp"))
}

fn replace_with_temp(tmp: &Path, path: &Path) -> std::io::Result<()> {
    match std::fs::rename(tmp, path) {
        Ok(()) => Ok(()),
        Err(first_error) if path.exists() => {
            std::fs::remove_file(path)?;
            std::fs::rename(tmp, path).map_err(|second_error| {
                let _ = std::fs::remove_file(tmp);
                if second_error.kind() == std::io::ErrorKind::Other {
                    first_error
                } else {
                    second_error
                }
            })
        }
        Err(error) => {
            let _ = std::fs::remove_file(tmp);
            Err(error)
        }
    }
}

fn write_text_atomic(path: &Path, after: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = atomic_temp_path(path);
    std::fs::write(&tmp, after)?;
    replace_with_temp(&tmp, path)
}

/// Apply the install to disk and persist an [`InstallState`].
///
/// Ordering matters for the never-clobber guarantee: we compute the **entire**
/// plan first (re-running every `apply(...)`), so a malformed config aborts here
/// — before a single byte is written. Only once the plan is known good do we
/// back up each target, write the new text, and record exactly what we changed
/// into the state so [`apply_uninstall`] can later undo precisely those edits.
///
/// Scheduled-task registration is the CLI's responsibility, not ours.
pub fn apply_install(
    t: &InstallTargets,
    agents: &AgentPresence,
    now: DateTime<Utc>,
) -> anyhow::Result<InstallState> {
    // Abort-before-write: compute every selected agent's edit first. Only after
    // all parse/shape checks pass do we touch any user config file.
    let ipc_dir = t.ipc_dir.to_string_lossy().into_owned();
    let ai_home = t.home.to_string_lossy().into_owned();
    let exe = t.exe.to_string_lossy().into_owned();

    let codex_pending = if agents.codex {
        let hooks_existing = read_existing(&t.codex_hooks)?;
        let (hooks_after, hooks_events) = codex_hooks::apply(hooks_existing.as_deref(), &exe)?;
        let config_existing = read_existing(&t.codex_config)?;
        let config_edit = codex_config::apply(config_existing.as_deref(), &ipc_dir, &ai_home)?;
        Some((hooks_after, hooks_events, config_edit))
    } else {
        None
    };

    let claude_pending = if agents.claude {
        let settings_existing = read_existing(&t.claude_settings)?;
        let (settings_after, settings_events) = claude::apply(settings_existing.as_deref(), &exe)?;
        // statusLine is a separate ROOT key; feed the hooks-applied JSON through
        // claude_statusline::apply so both edits land in a single write. Parse
        // happens here, before any write, preserving abort-before-write.
        let (settings_after_all, sl_apply) =
            claude_statusline::apply(Some(&settings_after), &exe)?;
        Some((settings_after_all, settings_events, sl_apply))
    } else {
        None
    };

    let mut st = InstallState {
        installed_at: now.to_rfc3339(),
        ..Default::default()
    };

    if let Some((hooks_after, hooks_events, config_edit)) = codex_pending {
        // Prior install-state, so an idempotent re-install accumulates ownership
        // instead of overwriting it (see the created_* merge below).
        let prior = state::load(&t.home);

        let hooks_backup = write_with_backup(&t.codex_hooks, &hooks_after, now)?;
        let config_backup = write_with_backup(&t.codex_config, &config_edit.text, now)?;

        st.codex.hooks_file = Some(FileMod {
            path: t.codex_hooks.to_string_lossy().into_owned(),
            backup: hooks_backup,
        });
        st.codex.config_file = Some(FileMod {
            path: t.codex_config.to_string_lossy().into_owned(),
            backup: config_backup,
        });
        st.codex.managed_hook_events = hooks_events;
        // writable_root_added / env_key_added are recorded by presence inside
        // codex_config::apply, so they survive an idempotent re-install.
        st.codex.writable_root_added = config_edit.writable_root_added;
        st.codex.env_key_added = config_edit.env_key_added;
        // created_* gate whether uninstall drops a now-empty table. A re-install
        // reports created=false (the table already exists), so OR in any prior
        // record to avoid losing the "we created it" fact across re-installs.
        st.codex.created_sandbox_table =
            config_edit.created_sandbox_table || prior.codex.created_sandbox_table;
        st.codex.created_env_table =
            config_edit.created_env_table || prior.codex.created_env_table;
    }

    if let Some((settings_after_all, settings_events, sl_apply)) = claude_pending {
        // Prior install-state so an idempotent re-install keeps the originally
        // recorded statusLine `previous` instead of losing it (see merge below).
        let prior = state::load(&t.home);

        let settings_backup = write_with_backup(&t.claude_settings, &settings_after_all, now)?;

        st.claude.settings_file = Some(FileMod {
            path: t.claude_settings.to_string_lossy().into_owned(),
            backup: settings_backup,
        });
        st.claude.managed_hook_events = settings_events;
        // Idempotent previous-merge: when re-applying over our OWN statusLine
        // (current_was_ours), `apply` reports no foreign previous, so keep the
        // one we recorded on the first install; otherwise record the freshly
        // captured foreign value.
        st.claude.statusline = Some(ClaudeStatuslineState {
            previous: if sl_apply.current_was_ours {
                prior.claude.statusline.and_then(|s| s.previous)
            } else {
                sl_apply.previous
            },
            installed_command: sl_apply.installed_command,
        });
    }

    state::save(&t.home, &st)?;
    Ok(st)
}

/// Surgically remove our edits, driven entirely by the recorded [`InstallState`].
///
/// For every file we recorded, if it still exists we read the current content,
/// run the matching `remove(...)` (which strips only our managed entries while
/// preserving any edits the user made afterwards), and write the result back.
/// Propagates `remove` errors. Does **not** restore backups; files we created
/// that become empty (`{}`) are left in place — harmless.
pub fn apply_uninstall(_t: &InstallTargets, st: &InstallState) -> anyhow::Result<()> {
    // Codex hooks.json
    if let Some(fm) = &st.codex.hooks_file {
        let path = Path::new(&fm.path);
        if let Some(text) = read_existing(path)? {
            let cleaned = codex_hooks::remove(&text)?;
            write_text_atomic(path, &cleaned)?;
        }
    }

    // Codex config.toml
    if let Some(fm) = &st.codex.config_file {
        let path = Path::new(&fm.path);
        if let Some(text) = read_existing(path)? {
            let cleaned = codex_config::remove(&text, &st.codex)?;
            write_text_atomic(path, &cleaned)?;
        }
    }

    // Claude settings.json
    if let Some(fm) = &st.claude.settings_file {
        let path = Path::new(&fm.path);
        if let Some(text) = read_existing(path)? {
            let cleaned = claude::remove(&text)?;
            // Restore the user's prior statusLine (or drop ours) before writing.
            let cleaned = match &st.claude.statusline {
                Some(sl) => claude_statusline::remove(&cleaned, sl)?,
                None => cleaned,
            };
            write_text_atomic(path, &cleaned)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn install_then_user_edit_then_uninstall_preserves_user_edit() {
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        std::fs::create_dir_all(uh.join(".codex")).unwrap();
        std::fs::create_dir_all(uh.join(".claude")).unwrap();
        // seed a complex codex config from the fixture + a claude settings
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codex-config-complex.toml"
            ),
            uh.join(".codex/config.toml"),
        )
        .unwrap();
        std::fs::write(uh.join(".claude/settings.json"), r#"{"model":"opus"}"#).unwrap();
        let ai_home = uh.join("ai-home");
        let t = targets_for(
            uh,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        let agents = detect_agents(&t);
        let st = apply_install(&t, &agents, Utc::now()).unwrap();
        assert!(st.codex.created_sandbox_table);

        // user adds an unrelated writable root AFTER install
        let cfg = std::fs::read_to_string(uh.join(".codex/config.toml")).unwrap();
        let mut doc: toml_edit::DocumentMut = cfg.parse().unwrap();
        doc["sandbox_workspace_write"]["writable_roots"]
            .as_array_mut()
            .unwrap()
            .push("C:/user/root");
        std::fs::write(uh.join(".codex/config.toml"), doc.to_string()).unwrap();

        apply_uninstall(&t, &st).unwrap();
        let final_cfg = std::fs::read_to_string(uh.join(".codex/config.toml")).unwrap();
        let fdoc: toml_edit::DocumentMut = final_cfg.parse().unwrap();
        // our ipc root gone, user's root survives, unrelated tables intact
        let roots = fdoc["sandbox_workspace_write"]["writable_roots"]
            .as_array()
            .unwrap();
        assert!(roots.iter().any(|v| v.as_str() == Some("C:/user/root")));
        assert!(roots
            .iter()
            .all(|v| !v.as_str().unwrap().contains("ai-home")));
        assert_eq!(fdoc["windows"]["sandbox"].as_str(), Some("unelevated"));
        // claude model preserved, our hooks gone
        let cs: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(uh.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(cs["model"], "opus");
    }

    #[test]
    fn reinstall_then_uninstall_removes_managed_entries() {
        // Regression: an idempotent SECOND install must not drop ownership of the
        // writable root / env key (codex_config records them by presence) nor of
        // the created_* table flags (apply_install ORs in the prior install's
        // record), or uninstall would orphan our entries in config.toml.
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        std::fs::create_dir_all(uh.join(".codex")).unwrap();
        std::fs::create_dir_all(uh.join(".claude")).unwrap();
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codex-config-complex.toml"
            ),
            uh.join(".codex/config.toml"),
        )
        .unwrap();
        std::fs::write(uh.join(".claude/settings.json"), r#"{"model":"opus"}"#).unwrap();
        let ai_home = uh.join("ai-home");
        let t = targets_for(
            uh,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        let agents = detect_agents(&t);

        // Install twice; the second run is idempotent over already-applied config.
        apply_install(&t, &agents, Utc::now()).unwrap();
        let st = apply_install(&t, &agents, Utc::now()).unwrap();

        // Ownership must survive the idempotent re-install.
        let ipc = ai_home.join("ipc").to_string_lossy().into_owned();
        assert_eq!(st.codex.writable_root_added.as_deref(), Some(ipc.as_str()));
        assert_eq!(st.codex.env_key_added.as_deref(), Some("AI_HANDOFF_HOME"));
        assert!(st.codex.created_sandbox_table);
        assert!(st.codex.created_env_table);

        apply_uninstall(&t, &st).unwrap();
        let final_cfg = std::fs::read_to_string(uh.join(".codex/config.toml")).unwrap();
        // "ai-home" appears only in our ipc root + AI_HANDOFF_HOME value; both gone.
        assert!(
            !final_cfg.contains("ai-home"),
            "managed entries orphaned after reinstall+uninstall:\n{final_cfg}"
        );
        assert!(!final_cfg.contains("AI_HANDOFF_HOME"));
        // empty tables we created are dropped
        assert!(!final_cfg.contains("sandbox_workspace_write"));
        assert!(!final_cfg.contains("shell_environment_policy"));
    }

    #[test]
    fn install_reinstall_uninstall_preserves_foreign_statusline() {
        // The highest-value statusLine test: a user with a pre-existing
        // statusLine installs, re-installs (idempotent), then uninstalls. Their
        // original statusLine must be restored exactly, and our managed hooks
        // must be gone. Mirrors the codex survival/reinstall tests above.
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        std::fs::create_dir_all(uh.join(".claude")).unwrap();
        let foreign = r#"{"model":"opus","statusLine":{"type":"command","command":"my-prompt --fancy","padding":1}}"#;
        std::fs::write(uh.join(".claude/settings.json"), foreign).unwrap();
        let ai_home = uh.join("ai-home");
        let t = targets_for(
            uh,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        let agents = AgentPresence {
            codex: false,
            claude: true,
        };

        // Install twice; the second run is idempotent over our own statusLine.
        apply_install(&t, &agents, Utc::now()).unwrap();
        let st = apply_install(&t, &agents, Utc::now()).unwrap();

        // After install our statusLine is live; the foreign one is recorded.
        let installed: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(uh.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            installed["statusLine"]["command"],
            "\"C:/p/ai-handoff.exe\" statusline"
        );
        assert_eq!(installed["statusLine"]["refreshInterval"], 15);
        // Idempotent re-install must keep the ORIGINAL foreign previous.
        let recorded = st.claude.statusline.as_ref().unwrap();
        assert_eq!(
            recorded.previous.as_ref().unwrap()["command"],
            "my-prompt --fancy"
        );

        apply_uninstall(&t, &st).unwrap();
        let restored: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(uh.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        // Foreign statusLine restored exactly (including extra keys), hooks gone.
        assert_eq!(restored["statusLine"]["command"], "my-prompt --fancy");
        assert_eq!(restored["statusLine"]["padding"], 1);
        assert_eq!(restored["model"], "opus");
        assert!(restored.get("hooks").is_none());
    }

    #[test]
    fn install_uninstall_removes_statusline_when_user_had_none() {
        // No prior statusLine → uninstall must DELETE our key, not leave it.
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        std::fs::create_dir_all(uh.join(".claude")).unwrap();
        std::fs::write(uh.join(".claude/settings.json"), r#"{"model":"opus"}"#).unwrap();
        let ai_home = uh.join("ai-home");
        let t = targets_for(
            uh,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        let agents = AgentPresence {
            codex: false,
            claude: true,
        };

        let st = apply_install(&t, &agents, Utc::now()).unwrap();
        apply_uninstall(&t, &st).unwrap();
        let restored: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(uh.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert!(restored.get("statusLine").is_none());
        assert_eq!(restored["model"], "opus");
    }

    #[test]
    fn install_aborts_before_any_write_when_later_agent_config_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        std::fs::create_dir_all(uh.join(".codex")).unwrap();
        std::fs::create_dir_all(uh.join(".claude")).unwrap();

        let codex_config = "sandbox_mode = \"workspace-write\"\n";
        let codex_hooks =
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"other"}]}]}}"#;
        let malformed_claude = "{ not valid json";
        std::fs::write(uh.join(".codex/config.toml"), codex_config).unwrap();
        std::fs::write(uh.join(".codex/hooks.json"), codex_hooks).unwrap();
        std::fs::write(uh.join(".claude/settings.json"), malformed_claude).unwrap();

        let ai_home = uh.join("ai-home");
        let t = targets_for(
            uh,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        let agents = AgentPresence {
            codex: true,
            claude: true,
        };

        let err = apply_install(&t, &agents, Utc::now()).unwrap_err();
        assert!(err.to_string().contains("key must be a string"));
        assert_eq!(
            std::fs::read_to_string(uh.join(".codex/config.toml")).unwrap(),
            codex_config
        );
        assert_eq!(
            std::fs::read_to_string(uh.join(".codex/hooks.json")).unwrap(),
            codex_hooks
        );
        assert_eq!(
            std::fs::read_to_string(uh.join(".claude/settings.json")).unwrap(),
            malformed_claude
        );
        assert!(!state_path(&ai_home).exists());
    }
}
