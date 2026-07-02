use ai_handoff_cli::commands::{handoff, hook};
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
fn stop_then_peer_session_start_roundtrips_capsule() {
    let _guard = lock();
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    std::env::set_var("AI_HANDOFF_HOME", home.path());
    ai_handoff_daemon::ensure_runtime_dirs().unwrap();

    let _worker = std::thread::spawn(|| {
        let router = Router::new();
        loop {
            serve_once(&router);
            std::thread::sleep(Duration::from_millis(10));
        }
    });

    let stop_payload = format!(
        "{}{}{}",
        r#"{"cwd":""#,
        cwd.path().to_string_lossy().replace('\\', "\\\\"),
        r#"","session_id":"codex-s","turn_id":"t1","last_assistant_message":"```ai-handoff-capsule\n{\"goal\":\"handoff e2e\",\"remaining\":[\"verify\"],\"next_prompt\":\"continue\"}\n```"}"#
    );
    let mut stop_out = Vec::new();
    let stop_code = hook::run_io(
        "stop",
        "codex",
        &mut Cursor::new(stop_payload.as_bytes()),
        &mut stop_out,
    );
    assert_eq!(stop_code, 0);
    assert_eq!(String::from_utf8(stop_out).unwrap(), "{}\n");

    let project_id = ai_handoff_core::fingerprint::fingerprint(cwd.path());
    let deadline = Instant::now() + Duration::from_secs(2);
    while ai_handoff_daemon::store::find_pending(&project_id).is_none() && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let pending = ai_handoff_daemon::store::find_pending(&project_id).unwrap();
    assert_eq!(
        pending.consumption.state,
        ai_handoff_core::capsule::ConsumptionState::Pending
    );
    assert_eq!(
        pending.source_agent,
        ai_handoff_core::capsule::AgentKind::Codex
    );

    // Session start only announces the pending capsule; it must NOT consume it.
    let start_payload = format!(
        r#"{{"cwd":"{}","session_id":"claude-s","turn_id":"t2"}}"#,
        cwd.path().to_string_lossy().replace('\\', "\\\\")
    );
    let mut start_out = Vec::new();
    let start_code = hook::run_io(
        "session-start",
        "claude-code",
        &mut Cursor::new(start_payload.as_bytes()),
        &mut start_out,
    );
    assert_eq!(start_code, 0);
    let start_text = String::from_utf8(start_out).unwrap();
    assert!(start_text.contains("additionalContext"));
    assert!(start_text.contains("/handoff"));

    let capsule_path = ai_handoff_core::paths::capsule_path(&project_id, &pending.capsule_id);
    let after_start: ai_handoff_core::capsule::Capsule =
        serde_json::from_slice(&std::fs::read(&capsule_path).unwrap()).unwrap();
    assert_eq!(
        after_start.consumption.state,
        ai_handoff_core::capsule::ConsumptionState::Pending
    );

    // Explicit /handoff (ai-handoff handoff) consumes it and prints the capsule.
    let prev_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(cwd.path()).unwrap();
    let mut consume_out = Vec::new();
    let consume_code = handoff::run_io("claude-code", &mut consume_out, false);
    std::env::set_current_dir(prev_dir).unwrap();
    assert_eq!(consume_code, 0);
    let consume_text = String::from_utf8(consume_out).unwrap();
    assert!(consume_text.contains("additionalContext"));
    assert!(consume_text.contains("handoff e2e"));

    let consumed: ai_handoff_core::capsule::Capsule =
        serde_json::from_slice(&std::fs::read(&capsule_path).unwrap()).unwrap();
    assert_eq!(
        consumed.consumption.state,
        ai_handoff_core::capsule::ConsumptionState::Consumed
    );
    std::env::remove_var("AI_HANDOFF_HOME");
}
