use ai_handoff_cli::commands::{checkpoint, doctor, handoff};
use ai_handoff_core::install::{state, InstallState, PluginRecord};
use ai_handoff_daemon::router::Router;
use ai_handoff_ipc::server::serve_once;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

#[test]
fn doctor_json_reports_daemon_unreachable_and_exits_zero() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());
    let mut out = Vec::new();
    let code = doctor::run_io(true, &mut out);
    assert_eq!(code, 0);
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(report["daemon"], "unreachable");
    assert!(
        matches!(
            report["ipc_permissions"]["status"].as_str(),
            Some("ok" | "warning")
        ),
        "{report}"
    );
    // Store write probe from the doctor process itself.
    assert!(
        matches!(
            report["store_permissions"]["status"].as_str(),
            Some("ok" | "warning" | "missing")
        ),
        "{report}"
    );
    // No daemon answered the ping → its store health is unknown, not false.
    assert!(report["daemon_store_writable"].is_null(), "{report}");
    std::env::remove_var("AI_HANDOFF_HOME");
}

#[test]
fn doctor_reports_ipc_subdir_health_missing_then_ok() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());

    // Fresh home: doctor's own ping creates the requests dir on write (the
    // client does that so hooks work before the daemon's first run), but the
    // responses dir only appears once a daemon answers — so it reads missing.
    let mut out = Vec::new();
    doctor::run_io(true, &mut out);
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(report["ipc_requests"]["status"], "ok", "{report}");
    assert_eq!(report["ipc_responses"]["status"], "missing", "{report}");

    // After the runtime dirs exist (what daemon startup does), both must be
    // writable and inheriting.
    ai_handoff_daemon::ensure_runtime_dirs().unwrap();
    ai_handoff_core::secure_fs::ensure_inherited_subdir(&ai_handoff_core::paths::requests_dir())
        .unwrap();
    ai_handoff_core::secure_fs::ensure_inherited_subdir(&ai_handoff_core::paths::responses_dir())
        .unwrap();
    let mut out = Vec::new();
    doctor::run_io(true, &mut out);
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(report["ipc_requests"]["status"], "ok", "{report}");
    assert_eq!(report["ipc_responses"]["status"], "ok", "{report}");

    std::env::remove_var("AI_HANDOFF_HOME");
}

#[cfg(windows)]
#[test]
fn doctor_flags_hardened_ipc_subdirs_as_broken() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());

    // Reproduce the old bug: subdirs hardened with /inheritance:r. The IPC
    // root check alone said "private" and missed this; the subdir check must
    // flag it.
    ai_handoff_core::secure_fs::ensure_private_dir(&ai_handoff_core::paths::requests_dir())
        .unwrap();
    ai_handoff_core::secure_fs::ensure_private_dir(&ai_handoff_core::paths::responses_dir())
        .unwrap();

    // Some sandboxed Windows test environments cannot disable inheritance
    // (icacls returns Access denied). In that case this fixture did not
    // reproduce the old bug, so the doctor must not be expected to flag it.
    if matches!(
        ai_handoff_core::secure_fs::inherited_subdir_status(
            &ai_handoff_core::paths::requests_dir()
        )
        .status,
        ai_handoff_core::secure_fs::PermissionStatus::Ok
    ) {
        std::env::remove_var("AI_HANDOFF_HOME");
        return;
    }

    let mut out = Vec::new();
    doctor::run_io(true, &mut out);
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(report["ipc_requests"]["status"], "warning", "{report}");
    assert_eq!(report["ipc_responses"]["status"], "warning", "{report}");

    std::env::remove_var("AI_HANDOFF_HOME");
}

#[test]
fn doctor_fix_repairs_runtime_dirs_and_reports_next_steps() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());

    // Keep the daemon-serving side alive so --fix's spawned daemon (or the
    // report ping) can be answered locally without a real spawned binary.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_worker = stop.clone();
    let worker = std::thread::spawn(move || {
        let router = Router::new();
        while !stop_worker.load(std::sync::atomic::Ordering::Relaxed) {
            serve_once(&router);
            std::thread::sleep(Duration::from_millis(5));
        }
    });

    let mut out = Vec::new();
    let code = doctor::run_io_fix(true, true, &mut out);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    worker.join().unwrap();
    assert_eq!(code, 0);
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    // The fix pass must have (re)created the whole IPC tree.
    assert_eq!(report["ipc_requests"]["status"], "ok", "{report}");
    assert_eq!(report["ipc_responses"]["status"], "ok", "{report}");
    let fixes = report["fixes"].as_array().unwrap();
    assert!(
        fixes
            .iter()
            .any(|fix| fix.as_str().unwrap_or("").contains("runtime directories")),
        "{report}"
    );
    // No plugins are installed in this fixture: doctor must say what to do.
    assert!(
        report["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step
                .as_str()
                .unwrap_or("")
                .contains("ai-handoff install --yes")),
        "{report}"
    );

    std::env::remove_var("AI_HANDOFF_HOME");
}

#[test]
fn doctor_reports_codex_trust_next_step_when_untrusted() {
    let _guard = lock();
    let user_home = tempfile::tempdir().unwrap();
    let ai_home = user_home.path().join("ai-home");
    let codex_root = user_home
        .path()
        .join(".agents")
        .join("plugins")
        .join("ai-handoff");
    std::fs::create_dir_all(codex_root.join(".codex-plugin")).unwrap();
    std::fs::write(codex_root.join(".codex-plugin/plugin.json"), "{}").unwrap();
    let marketplace = user_home
        .path()
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    std::fs::write(&marketplace, "{}").unwrap();
    let codex_config = user_home.path().join(".codex").join("config.toml");
    std::fs::create_dir_all(codex_config.parent().unwrap()).unwrap();
    // Plugin enabled, but no trusted hook hashes recorded.
    std::fs::write(
        &codex_config,
        "[plugins.\"ai-handoff@claude-codex-auto-handoff\"]\nenabled = true\n",
    )
    .unwrap();
    state::save(
        &ai_home,
        &InstallState {
            codex: state::CodexState {
                plugin: Some(PluginRecord {
                    root: codex_root.to_string_lossy().into_owned(),
                    files: vec![],
                    marketplace_file: Some(marketplace.to_string_lossy().into_owned()),
                }),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();

    std::env::set_var("AI_HANDOFF_HOME", &ai_home);
    let mut out = Vec::new();
    doctor::run_io(true, &mut out);
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(report["plugin"]["codex"]["trusted"], false, "{report}");
    assert!(
        report["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().unwrap_or("").contains("/hooks")),
        "{report}"
    );
    std::env::remove_var("AI_HANDOFF_HOME");
}

#[test]
fn doctor_json_reports_plugin_install_enable_and_trust_state() {
    let _guard = lock();
    let user_home = tempfile::tempdir().unwrap();
    let ai_home = user_home.path().join("ai-home");
    let claude_root = user_home
        .path()
        .join(".claude")
        .join("skills")
        .join("ai-handoff");
    let codex_root = user_home
        .path()
        .join(".agents")
        .join("plugins")
        .join("ai-handoff");
    std::fs::create_dir_all(claude_root.join(".claude-plugin")).unwrap();
    std::fs::create_dir_all(codex_root.join(".codex-plugin")).unwrap();
    std::fs::write(claude_root.join(".claude-plugin/plugin.json"), "{}").unwrap();
    std::fs::write(codex_root.join(".codex-plugin/plugin.json"), "{}").unwrap();

    let marketplace = user_home
        .path()
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    std::fs::create_dir_all(marketplace.parent().unwrap()).unwrap();
    std::fs::write(&marketplace, "{}").unwrap();
    let codex_config = user_home.path().join(".codex").join("config.toml");
    std::fs::create_dir_all(codex_config.parent().unwrap()).unwrap();
    std::fs::write(
        &codex_config,
        r#"[plugins."ai-handoff@claude-codex-auto-handoff"]
enabled = true

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:SessionStart:0:0"]
trusted_hash = "sha256:trusted-v2"

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:UserPromptSubmit:0:0"]
trusted_hash = "sha256:trusted-v2"

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:PostToolUse:0:0"]
trusted_hash = "sha256:trusted-v2"

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:Stop:0:0"]
trusted_hash = "sha256:trusted-v2"
"#,
    )
    .unwrap();

    state::save(
        &ai_home,
        &InstallState {
            claude: state::ClaudeState {
                plugin: Some(PluginRecord {
                    root: claude_root.to_string_lossy().into_owned(),
                    files: vec![],
                    marketplace_file: None,
                }),
                ..Default::default()
            },
            codex: state::CodexState {
                plugin: Some(PluginRecord {
                    root: codex_root.to_string_lossy().into_owned(),
                    files: vec![],
                    marketplace_file: Some(marketplace.to_string_lossy().into_owned()),
                }),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();

    std::env::set_var("AI_HANDOFF_HOME", &ai_home);
    let mut out = Vec::new();
    let code = doctor::run_io(true, &mut out);
    assert_eq!(code, 0);
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(report["plugin"]["claude"]["installed"], true);
    assert_eq!(report["plugin"]["claude"]["enabled"], true);
    assert_eq!(report["plugin"]["codex"]["installed"], true);
    assert_eq!(report["plugin"]["codex"]["enabled"], true);
    assert_eq!(report["plugin"]["codex"]["trusted"], true);
    std::env::remove_var("AI_HANDOFF_HOME");
}

#[test]
fn checkpoint_with_daemon_online_writes_capsule() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let previous_cwd = std::env::current_dir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());
    std::env::set_current_dir(cwd.path()).unwrap();
    ai_handoff_daemon::ensure_runtime_dirs().unwrap();

    let worker = std::thread::spawn(|| {
        let router = Router::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if serve_once(&router) > 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("daemon did not process checkpoint request");
    });

    let mut out = Vec::new();
    let code = checkpoint::run_io(Some("manual checkpoint".into()), None, None, "", &mut out);
    worker.join().unwrap();
    assert_eq!(code, 0);
    let project_id = ai_handoff_core::fingerprint::fingerprint(cwd.path());
    let pending = ai_handoff_daemon::store::find_pending(&project_id, "claude-code").unwrap();
    assert_eq!(pending.summary.goal, "manual checkpoint");
    assert_eq!(
        std::fs::read_to_string(
            ai_handoff_core::paths::project_dir(&project_id).join("project.label")
        )
        .unwrap(),
        cwd.path().file_name().unwrap().to_string_lossy()
    );
    std::env::set_current_dir(previous_cwd).unwrap();
    std::env::remove_var("AI_HANDOFF_HOME");
}

#[test]
fn handoff_offline_prints_empty_json_and_exits_nonzero() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());

    let mut out = Vec::new();
    let code = handoff::run_io("claude-code", false, false, None, &mut out, false);
    // No daemon: stdout keeps the `{}` shape skills parse, but the exit code
    // must say the handoff did not happen.
    assert_eq!(code, 1);
    assert_eq!(String::from_utf8(out).unwrap().trim(), "{}");
    std::env::remove_var("AI_HANDOFF_HOME");
}

#[test]
fn checkpoint_offline_reports_daemon_unavailable_in_output() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());

    let mut out = Vec::new();
    let code = checkpoint::run_io(Some("offline checkpoint".into()), None, None, "", &mut out);
    assert_eq!(code, 1);
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(report["status"], "degraded");
    assert_eq!(report["warnings"][0], "daemon_unavailable");

    std::env::remove_var("AI_HANDOFF_HOME");
}

#[test]
fn checkpoint_structured_stdin_respects_capsule_limits() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let previous_cwd = std::env::current_dir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());
    std::env::set_current_dir(cwd.path()).unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "[capsule]\ndone_max_items = 1\nremaining_max_items = 2\nrisks_max_items = 1\nnext_prompt_max_items = 2\n",
    )
    .unwrap();
    ai_handoff_daemon::ensure_runtime_dirs().unwrap();

    let worker = std::thread::spawn(|| {
        let router = Router::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if serve_once(&router) > 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("daemon did not process checkpoint request");
    });

    let payload = r#"{"goal":"structured checkpoint","done":["a","b"],"remaining":["c","d","e"],"risks":["f","g"],"next_prompt":"one | two | three"}"#;
    let mut out = Vec::new();
    let code = checkpoint::run_io(None, None, None, payload, &mut out);
    worker.join().unwrap();
    assert_eq!(code, 0);
    let project_id = ai_handoff_core::fingerprint::fingerprint(cwd.path());
    let pending = ai_handoff_daemon::store::find_pending(&project_id, "claude-code").unwrap();
    assert_eq!(pending.summary.goal, "structured checkpoint");
    assert_eq!(pending.summary.done, vec!["a"]);
    assert_eq!(pending.summary.remaining, vec!["c", "d"]);
    assert_eq!(pending.summary.risks, vec!["f"]);
    assert_eq!(pending.next_prompt.as_deref(), Some("one | two"));
    std::env::set_current_dir(previous_cwd).unwrap();
    std::env::remove_var("AI_HANDOFF_HOME");
}

#[test]
fn checkpoint_agent_flag_sets_source_and_target_stays_open() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let previous_cwd = std::env::current_dir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());
    std::env::set_current_dir(cwd.path()).unwrap();
    ai_handoff_daemon::ensure_runtime_dirs().unwrap();

    let worker = std::thread::spawn(|| {
        let router = Router::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if serve_once(&router) > 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("daemon did not process checkpoint request");
    });

    let mut out = Vec::new();
    // --agent records the author; without --target the capsule stays open
    // (any agent may consume) instead of guessing "the opposite agent".
    let code = checkpoint::run_io(
        Some("from claude".into()),
        Some("claude-code".into()),
        None,
        "",
        &mut out,
    );
    worker.join().unwrap();
    assert_eq!(code, 0);
    let project_id = ai_handoff_core::fingerprint::fingerprint(cwd.path());
    let pending = ai_handoff_daemon::store::find_pending(&project_id, "codex").unwrap();
    assert_eq!(pending.source_agent, "claude-code");
    assert_eq!(pending.target_agent, None);
    std::env::set_current_dir(previous_cwd).unwrap();
    std::env::remove_var("AI_HANDOFF_HOME");
}

#[test]
fn checkpoint_target_flag_routes_capsule_to_named_agent() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let previous_cwd = std::env::current_dir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());
    std::env::set_current_dir(cwd.path()).unwrap();
    ai_handoff_daemon::ensure_runtime_dirs().unwrap();

    let worker = std::thread::spawn(|| {
        let router = Router::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if serve_once(&router) > 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("daemon did not process checkpoint request");
    });

    let mut out = Vec::new();
    let code = checkpoint::run_io(
        Some("for grok".into()),
        Some("claude-code".into()),
        Some("grok".into()),
        "",
        &mut out,
    );
    worker.join().unwrap();
    assert_eq!(code, 0);
    let project_id = ai_handoff_core::fingerprint::fingerprint(cwd.path());
    // Targeted at grok: grok claims it, codex does not see it.
    assert!(ai_handoff_daemon::store::find_pending(&project_id, "codex").is_none());
    let pending = ai_handoff_daemon::store::find_pending(&project_id, "grok").unwrap();
    assert_eq!(pending.source_agent, "claude-code");
    assert_eq!(pending.target_agent.as_deref(), Some("grok"));
    std::env::set_current_dir(previous_cwd).unwrap();
    std::env::remove_var("AI_HANDOFF_HOME");
}
