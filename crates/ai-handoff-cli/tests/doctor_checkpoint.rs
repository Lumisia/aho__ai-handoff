use ai_handoff_cli::commands::{checkpoint, doctor};
use ai_handoff_core::install::{state, InstallState, PluginRecord};
use ai_handoff_daemon::router::Router;
use ai_handoff_ipc::server::serve_once;
use std::io::Cursor;
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
    let code = checkpoint::run_io(
        Some("manual checkpoint".into()),
        &mut Cursor::new(Vec::new()),
        &mut out,
    );
    worker.join().unwrap();
    assert_eq!(code, 0);
    let project_id = ai_handoff_core::fingerprint::fingerprint(cwd.path());
    let pending = ai_handoff_daemon::store::find_pending(&project_id).unwrap();
    assert_eq!(pending.summary.goal, "manual checkpoint");
    std::env::set_current_dir(previous_cwd).unwrap();
    std::env::remove_var("AI_HANDOFF_HOME");
}
