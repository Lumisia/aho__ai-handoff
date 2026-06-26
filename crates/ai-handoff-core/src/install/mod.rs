pub mod backup;
pub mod claude;
pub mod claude_statusline;
pub mod codex_config;
pub mod codex_hooks;
pub mod detect;
pub mod diff;
pub mod duplicate;
pub mod plugin;
pub mod state;

pub use backup::{backup_file, backup_path};
pub use detect::{detect_agents, targets_for, AgentPresence, InstallTargets};
pub use plugin::{generate_bundle, merge_marketplace_entry, remove_marketplace_entry};
pub use state::{
    load, save, state_path, AutostartKind, AutostartState, ClaudeState, ClaudeStatuslineState,
    CodexState, FileMod, InstallState, PluginRecord,
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
    plugin: bool,
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
    let codex_hooks_existing = if agents.codex {
        read_existing(&t.codex_hooks)?
    } else {
        None
    };
    let claude_settings_existing = if agents.claude {
        read_existing(&t.claude_settings)?
    } else {
        None
    };

    if agents.codex {
        // Codex config.toml (writable_roots/env in both modes; plugin enable on top).
        let edit = codex_config::apply(codex_config_existing.as_deref(), &ipc_dir, &ai_home)?;
        let config_after = if plugin {
            codex_config::enable_plugin(Some(&edit.text))?
        } else {
            edit.text
        };
        file_plans.push(diff::FilePlan {
            path: t.codex_config.to_string_lossy().into_owned(),
            before: codex_config_existing.clone(),
            after: config_after,
        });

        if plugin {
            // Marketplace registration replaces the direct hooks.json patch.
            let marketplace_existing = read_existing(&t.agents_marketplace)?;
            let marketplace_after =
                plugin::merge_marketplace_entry(marketplace_existing.as_deref())?;
            file_plans.push(diff::FilePlan {
                path: t.agents_marketplace.to_string_lossy().into_owned(),
                before: marketplace_existing,
                after: marketplace_after,
            });
        } else {
            // Legacy: direct Codex hooks.json patch.
            let (hooks_after, _events) = codex_hooks::apply(codex_hooks_existing.as_deref(), &exe)?;
            file_plans.push(diff::FilePlan {
                path: t.codex_hooks.to_string_lossy().into_owned(),
                before: codex_hooks_existing.clone(),
                after: hooks_after,
            });
        }
    }

    if agents.claude {
        // In plugin mode the bundle carries hooks, so settings only gets the
        // statusLine; in legacy mode we preview the settings `hooks` patch too.
        let settings_base = if plugin {
            claude_settings_existing.clone()
        } else {
            let (after, _events) = claude::apply(claude_settings_existing.as_deref(), &exe)?;
            Some(after)
        };
        let (settings_after_all, _sl_apply) =
            claude_statusline::apply(settings_base.as_deref(), &exe)?;
        file_plans.push(diff::FilePlan {
            path: t.claude_settings.to_string_lossy().into_owned(),
            before: claude_settings_existing.clone(),
            after: settings_after_all,
        });
    }

    let duplicates = duplicate::detect(
        codex_config_existing.as_deref(),
        codex_hooks_existing.as_deref(),
        claude_settings_existing.as_deref(),
        plugin,
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
/// `plugin` selects how lifecycle hooks are delivered:
/// - `true` (default): drop a generated plugin bundle per agent (Claude
///   auto-loads `~/.claude/skills/ai-handoff`; Codex gets a bundle in
///   `~/.agents/plugins/ai-handoff` + a `marketplace.json` entry + the
///   `[plugins."ai-handoff@..."] enabled = true` toggle). The direct
///   `hooks.json` / settings `hooks` patches are NOT written.
/// - `false` (`--no-plugin`): the legacy behavior — patch Codex `hooks.json`
///   and Claude settings `hooks` directly, with NO bundle or marketplace work.
///
/// In BOTH modes the Claude statusLine and the Codex `writable_roots` / env are
/// applied directly, exactly as before.
///
/// Scheduled-task registration is the CLI's responsibility, not ours.
pub fn apply_install(
    t: &InstallTargets,
    agents: &AgentPresence,
    now: DateTime<Utc>,
    plugin: bool,
) -> anyhow::Result<InstallState> {
    // Abort-before-write: compute every selected agent's edit first. Only after
    // all parse/shape checks pass do we touch any user config file.
    let ipc_dir = t.ipc_dir.to_string_lossy().into_owned();
    let ai_home = t.home.to_string_lossy().into_owned();
    let exe = t.exe.to_string_lossy().into_owned();

    // --- Codex: parse/compute everything (no writes yet) ---
    let codex_pending = if agents.codex {
        let config_existing = read_existing(&t.codex_config)?;
        // writable_roots + env are applied in BOTH modes.
        let config_edit = codex_config::apply(config_existing.as_deref(), &ipc_dir, &ai_home)?;

        if plugin {
            // Plugin mode: enable our bundle in config (over the just-applied
            // text so both edits land in one write), merge the marketplace
            // entry, and leave hooks.json untouched.
            let config_with_plugin = codex_config::enable_plugin(Some(&config_edit.text))?;
            let marketplace_existing = read_existing(&t.agents_marketplace)?;
            let marketplace_after =
                plugin::merge_marketplace_entry(marketplace_existing.as_deref())?;
            CodexPending::Plugin {
                config_edit,
                config_text: config_with_plugin,
                marketplace_after,
            }
        } else {
            // Legacy mode: patch hooks.json directly, no bundle/marketplace.
            let hooks_existing = read_existing(&t.codex_hooks)?;
            let (hooks_after, hooks_events) = codex_hooks::apply(hooks_existing.as_deref(), &exe)?;
            CodexPending::NoPlugin {
                config_edit,
                hooks_after,
                hooks_events,
            }
        }
    } else {
        CodexPending::Absent
    };

    // --- Claude: parse/compute everything (no writes yet) ---
    let claude_pending = if agents.claude {
        let settings_existing = read_existing(&t.claude_settings)?;
        // In plugin mode the bundle carries hooks, so settings only gets the
        // statusLine; in legacy mode we patch the settings `hooks` first.
        let (settings_base, settings_events) = if plugin {
            (settings_existing.clone(), Vec::new())
        } else {
            let (after, events) = claude::apply(settings_existing.as_deref(), &exe)?;
            (Some(after), events)
        };
        // statusLine is a separate ROOT key; feed the (optionally hooks-applied)
        // JSON through claude_statusline::apply so all edits land in a single
        // write. Parse happens here, before any write, preserving abort-before-write.
        let (settings_after_all, sl_apply) =
            claude_statusline::apply(settings_base.as_deref(), &exe)?;
        Some((settings_after_all, settings_events, sl_apply))
    } else {
        None
    };

    let mut st = InstallState {
        installed_at: now.to_rfc3339(),
        ..Default::default()
    };

    match codex_pending {
        CodexPending::Absent => {}
        CodexPending::NoPlugin {
            config_edit,
            hooks_after,
            hooks_events,
        } => {
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
            record_codex_config(&mut st, &config_edit, &prior);
        }
        CodexPending::Plugin {
            config_edit,
            config_text,
            marketplace_after,
        } => {
            let prior = state::load(&t.home);
            // Generate the bundle first; record its files for surgical uninstall.
            let mut record =
                generate_bundle(crate::capsule::AgentKind::Codex, &exe, &t.codex_plugin_dir)?;
            // Marketplace registration (never-clobber merge computed above).
            write_with_backup(&t.agents_marketplace, &marketplace_after, now)?;
            record.marketplace_file = Some(t.agents_marketplace.to_string_lossy().into_owned());
            // Config: writable_roots/env + `[plugins."ai-handoff@..."] enabled`.
            let config_backup = write_with_backup(&t.codex_config, &config_text, now)?;
            st.codex.config_file = Some(FileMod {
                path: t.codex_config.to_string_lossy().into_owned(),
                backup: config_backup,
            });
            st.codex.plugin = Some(record);
            record_codex_config(&mut st, &config_edit, &prior);
        }
    }

    if let Some((settings_after_all, settings_events, sl_apply)) = claude_pending {
        // Prior install-state so an idempotent re-install keeps the originally
        // recorded statusLine `previous` instead of losing it (see merge below).
        let prior = state::load(&t.home);

        if plugin {
            // Drop the auto-loaded bundle; record its files for surgical uninstall.
            let record = generate_bundle(
                crate::capsule::AgentKind::ClaudeCode,
                &exe,
                &t.claude_plugin_dir,
            )?;
            st.claude.plugin = Some(record);
        }

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

/// The computed-but-not-yet-written Codex edit, branched by install mode.
enum CodexPending {
    Absent,
    NoPlugin {
        config_edit: codex_config::ConfigEdit,
        hooks_after: String,
        hooks_events: Vec<String>,
    },
    Plugin {
        config_edit: codex_config::ConfigEdit,
        /// Full config text incl. the `[plugins."ai-handoff@..."]` enable.
        config_text: String,
        marketplace_after: String,
    },
}

/// Record the writable-root / env / created-table ownership into `st.codex`,
/// merging with `prior` so an idempotent re-install never loses ownership.
fn record_codex_config(
    st: &mut InstallState,
    config_edit: &codex_config::ConfigEdit,
    prior: &InstallState,
) {
    // writable_root_added / env_key_added are recorded by presence inside
    // codex_config::apply, so they survive an idempotent re-install.
    st.codex.writable_root_added = config_edit.writable_root_added.clone();
    st.codex.env_key_added = config_edit.env_key_added.clone();
    // created_* gate whether uninstall drops a now-empty table. A re-install
    // reports created=false (the table already exists), so OR in any prior
    // record to avoid losing the "we created it" fact across re-installs.
    st.codex.created_sandbox_table =
        config_edit.created_sandbox_table || prior.codex.created_sandbox_table;
    st.codex.created_env_table = config_edit.created_env_table || prior.codex.created_env_table;
}

/// Surgically remove our edits, driven entirely by the recorded [`InstallState`].
///
/// For every file we recorded, if it still exists we read the current content,
/// run the matching `remove(...)` (which strips only our managed entries while
/// preserving any edits the user made afterwards), and write the result back.
/// Propagates `remove` errors. Does **not** restore backups; files we created
/// that become empty (`{}`) are left in place — harmless.
///
/// Plugin bundles (recorded in `st.codex.plugin` / `st.claude.plugin`) are
/// deleted file-by-file, the `[plugins."ai-handoff@..."]` enable is dropped
/// from `config.toml`, and our entry is removed from the personal
/// `marketplace.json` — all preserving any foreign content.
pub fn apply_uninstall(_t: &InstallTargets, st: &InstallState) -> anyhow::Result<()> {
    // Codex hooks.json (legacy / --no-plugin install only).
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
            // Always strip our writable_root / env additions...
            let cleaned = codex_config::remove(&text, &st.codex)?;
            // ...and, when this was a plugin install, the `[plugins]` enable too.
            let cleaned = if st.codex.plugin.is_some() {
                codex_config::disable_plugin(&cleaned)?
            } else {
                cleaned
            };
            write_text_atomic(path, &cleaned)?;
        }
    }

    // Codex plugin bundle + marketplace entry.
    if let Some(rec) = &st.codex.plugin {
        remove_plugin_bundle(rec)?;
        if let Some(mp) = &rec.marketplace_file {
            let path = Path::new(mp);
            if let Some(text) = read_existing(path)? {
                let cleaned = remove_marketplace_entry(&text)?;
                write_text_atomic(path, &cleaned)?;
            }
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

    // Claude plugin bundle.
    if let Some(rec) = &st.claude.plugin {
        remove_plugin_bundle(rec)?;
    }

    Ok(())
}

/// Delete a recorded plugin bundle's files and prune the directory tree it left
/// behind (`<root>` and its now-empty subdirs). Missing files are not an error
/// — uninstall is idempotent and tolerant of partial prior cleanup.
fn remove_plugin_bundle(rec: &PluginRecord) -> std::io::Result<()> {
    let root = Path::new(&rec.root);
    // Removing the whole bundle root is correct: we own this directory (it is
    // `~/.claude/skills/ai-handoff` or `~/.agents/plugins/ai-handoff`, created
    // solely by us). This also clears the generated hooks + skills + manifest.
    if root.exists() {
        std::fs::remove_dir_all(root)?;
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
        let st = apply_install(&t, &agents, Utc::now(), false).unwrap();
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
        apply_install(&t, &agents, Utc::now(), false).unwrap();
        let st = apply_install(&t, &agents, Utc::now(), false).unwrap();

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
        apply_install(&t, &agents, Utc::now(), false).unwrap();
        let st = apply_install(&t, &agents, Utc::now(), false).unwrap();

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

        let st = apply_install(&t, &agents, Utc::now(), false).unwrap();
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

        let err = apply_install(&t, &agents, Utc::now(), false).unwrap_err();
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

    // -----------------------------------------------------------------------
    // Plugin-mode (default) install/uninstall
    // -----------------------------------------------------------------------

    fn plugin_targets(uh: &std::path::Path) -> (std::path::PathBuf, InstallTargets) {
        std::fs::create_dir_all(uh.join(".codex")).unwrap();
        std::fs::create_dir_all(uh.join(".claude")).unwrap();
        std::fs::write(
            uh.join(".codex/config.toml"),
            "sandbox_mode = \"workspace-write\"\n",
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
        (ai_home, t)
    }

    #[test]
    fn plugin_install_drops_bundles_registers_marketplace_and_skips_direct_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        let (_ai_home, t) = plugin_targets(uh);
        let agents = AgentPresence {
            codex: true,
            claude: true,
        };

        let st = apply_install(&t, &agents, Utc::now(), true).unwrap();

        // Bundles exist at both plugin dirs.
        assert!(t
            .claude_plugin_dir
            .join(".claude-plugin/plugin.json")
            .exists());
        assert!(t.claude_plugin_dir.join("hooks/hooks.json").exists());
        assert!(t.claude_plugin_dir.join("skills/handoff/SKILL.md").exists());
        assert!(t
            .codex_plugin_dir
            .join(".codex-plugin/plugin.json")
            .exists());
        assert!(t.codex_plugin_dir.join("hooks/hooks.json").exists());

        // Plugin records persisted.
        assert!(st.claude.plugin.is_some());
        let codex_rec = st.codex.plugin.as_ref().unwrap();
        assert_eq!(
            codex_rec.marketplace_file.as_deref(),
            Some(t.agents_marketplace.to_string_lossy().as_ref())
        );

        // marketplace.json registered with our entry.
        let mp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&t.agents_marketplace).unwrap()).unwrap();
        assert!(mp["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "ai-handoff"));

        // config.toml has the plugin enable AND the writable_roots/env edits.
        let cfg: toml_edit::DocumentMut = std::fs::read_to_string(&t.codex_config)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            cfg["plugins"][codex_config::PLUGIN_ENABLE_KEY]["enabled"].as_bool(),
            Some(true)
        );
        assert!(cfg["sandbox_workspace_write"]["writable_roots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().map(|s| s.contains("ipc")).unwrap_or(false)));
        assert_eq!(
            cfg["shell_environment_policy"]["set"]["AI_HANDOFF_HOME"]
                .as_str()
                .map(|s| s.contains("ai-home")),
            Some(true)
        );

        // NO direct codex hooks.json written, and no hooks_file recorded.
        assert!(!t.codex_hooks.exists());
        assert!(st.codex.hooks_file.is_none());

        // Claude settings has the statusLine but NO managed `hooks` entry.
        let cs: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&t.claude_settings).unwrap()).unwrap();
        assert_eq!(
            cs["statusLine"]["command"],
            "\"C:/p/ai-handoff.exe\" statusline"
        );
        assert!(cs.get("hooks").is_none());
        assert!(st.claude.managed_hook_events.is_empty());
    }

    #[test]
    fn plugin_uninstall_round_trip_preserves_foreign_marketplace_and_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        let (_ai_home, t) = plugin_targets(uh);
        let agents = AgentPresence {
            codex: true,
            claude: true,
        };

        // Pre-seed a FOREIGN marketplace entry and a FOREIGN [plugins] enable.
        std::fs::create_dir_all(t.agents_marketplace.parent().unwrap()).unwrap();
        std::fs::write(
            &t.agents_marketplace,
            r#"{"name":"mine","plugins":[{"name":"other","source":{"source":"url","url":"https://x"}}]}"#,
        )
        .unwrap();
        std::fs::write(
            &t.codex_config,
            "sandbox_mode = \"workspace-write\"\n[plugins.\"other@x\"]\nenabled = true\n",
        )
        .unwrap();

        let st = apply_install(&t, &agents, Utc::now(), true).unwrap();
        // Bundles present after install.
        assert!(t.claude_plugin_dir.exists());
        assert!(t.codex_plugin_dir.exists());

        apply_uninstall(&t, &st).unwrap();

        // Bundle dirs deleted.
        assert!(!t.claude_plugin_dir.exists());
        assert!(!t.codex_plugin_dir.exists());

        // Our marketplace entry gone, foreign one preserved.
        let mp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&t.agents_marketplace).unwrap()).unwrap();
        let plugins = mp["plugins"].as_array().unwrap();
        assert!(plugins.iter().all(|p| p["name"] != "ai-handoff"));
        assert!(plugins.iter().any(|p| p["name"] == "other"));
        assert_eq!(mp["name"], "mine");

        // Our [plugins] enable removed, foreign [plugins."other@x"] preserved.
        let cfg: toml_edit::DocumentMut = std::fs::read_to_string(&t.codex_config)
            .unwrap()
            .parse()
            .unwrap();
        assert!(cfg["plugins"]
            .get(codex_config::PLUGIN_ENABLE_KEY)
            .is_none());
        assert_eq!(cfg["plugins"]["other@x"]["enabled"].as_bool(), Some(true));
        // writable_roots/env also cleaned up.
        assert!(!std::fs::read_to_string(&t.codex_config)
            .unwrap()
            .contains("ai-home"));
    }

    #[test]
    fn plugin_reinstall_then_uninstall_removes_managed_plugin_state_once() {
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        let (_ai_home, t) = plugin_targets(uh);
        let agents = AgentPresence {
            codex: true,
            claude: true,
        };

        apply_install(&t, &agents, Utc::now(), true).unwrap();
        let st = apply_install(&t, &agents, Utc::now(), true).unwrap();

        // Reinstall is idempotent: one marketplace entry, one plugin enable,
        // no direct hooks.json fallback, and ownership still recorded.
        let mp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&t.agents_marketplace).unwrap()).unwrap();
        assert_eq!(
            mp["plugins"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["name"] == "ai-handoff")
                .count(),
            1
        );
        let cfg_text = std::fs::read_to_string(&t.codex_config).unwrap();
        let cfg: toml_edit::DocumentMut = cfg_text.parse().unwrap();
        assert_eq!(
            cfg["plugins"][codex_config::PLUGIN_ENABLE_KEY]["enabled"].as_bool(),
            Some(true)
        );
        assert!(!t.codex_hooks.exists());
        assert_eq!(
            st.codex.writable_root_added.as_deref(),
            Some(t.ipc_dir.to_string_lossy().as_ref())
        );
        assert_eq!(st.codex.env_key_added.as_deref(), Some("AI_HANDOFF_HOME"));
        assert!(st.codex.plugin.is_some());
        assert!(st.claude.plugin.is_some());

        apply_uninstall(&t, &st).unwrap();

        assert!(!t.claude_plugin_dir.exists());
        assert!(!t.codex_plugin_dir.exists());
        let cleaned_mp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&t.agents_marketplace).unwrap()).unwrap();
        assert!(cleaned_mp["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["name"] != "ai-handoff"));
        let cleaned_cfg = std::fs::read_to_string(&t.codex_config).unwrap();
        assert!(!cleaned_cfg.contains(codex_config::PLUGIN_ENABLE_KEY));
        assert!(!cleaned_cfg.contains("ai-home"));
        assert!(!cleaned_cfg.contains("AI_HANDOFF_HOME"));
    }

    #[test]
    fn no_plugin_install_writes_direct_hooks_and_no_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        let (_ai_home, t) = plugin_targets(uh);
        let agents = AgentPresence {
            codex: true,
            claude: true,
        };

        let st = apply_install(&t, &agents, Utc::now(), false).unwrap();

        // Direct codex hooks.json written + claude settings `hooks` managed entry.
        assert!(t.codex_hooks.exists());
        assert!(st.codex.hooks_file.is_some());
        let cs: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&t.claude_settings).unwrap()).unwrap();
        assert!(cs["hooks"]["Stop"][0]["hooks"][0]["_aiHandoff"]
            .as_bool()
            .unwrap());

        // NO plugin bundle, NO marketplace entry, NO [plugins] enable.
        assert!(st.codex.plugin.is_none());
        assert!(st.claude.plugin.is_none());
        assert!(!t.claude_plugin_dir.exists());
        assert!(!t.codex_plugin_dir.exists());
        assert!(!t.agents_marketplace.exists());
        let cfg = std::fs::read_to_string(&t.codex_config).unwrap();
        assert!(!cfg.contains("[plugins"));
    }

    #[test]
    fn plugin_install_aborts_on_malformed_marketplace_without_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        let (ai_home, t) = plugin_targets(uh);
        let agents = AgentPresence {
            codex: true,
            claude: true,
        };

        // Malformed marketplace.json must abort BEFORE any write.
        std::fs::create_dir_all(t.agents_marketplace.parent().unwrap()).unwrap();
        let malformed = "{ not valid json";
        std::fs::write(&t.agents_marketplace, malformed).unwrap();
        let pristine_cfg = std::fs::read_to_string(&t.codex_config).unwrap();

        let err = apply_install(&t, &agents, Utc::now(), true).unwrap_err();
        assert!(
            err.to_string().contains("marketplace.json parse error"),
            "unexpected error: {err}"
        );

        // Nothing clobbered: marketplace untouched, config untouched, no bundle,
        // no state file.
        assert_eq!(
            std::fs::read_to_string(&t.agents_marketplace).unwrap(),
            malformed
        );
        assert_eq!(
            std::fs::read_to_string(&t.codex_config).unwrap(),
            pristine_cfg
        );
        assert!(!t.codex_plugin_dir.exists());
        assert!(!t.claude_plugin_dir.exists());
        assert!(!state_path(&ai_home).exists());
    }
}
