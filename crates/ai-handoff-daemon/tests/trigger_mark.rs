use ai_handoff_core::capsule::AgentKind;
use ai_handoff_daemon::trigger_mark::{check_and_record_at, mark_path, FIVE_HOUR_WINDOW_MS};

#[test]
fn first_threshold_fires_then_same_window_suppresses_across_instances() {
    let home = tempfile::tempdir().unwrap();
    let first = check_and_record_at(home.path(), &AgentKind::Codex, 1_000, Some(10_000));
    assert!(first.fired);
    assert_eq!(first.expires_at_ms, 10_000);

    let second = check_and_record_at(home.path(), &AgentKind::Codex, 2_000, Some(10_000));
    assert!(!second.fired);
    assert_eq!(second.expires_at_ms, 10_000);
}

#[test]
fn expired_window_fires_again_and_uses_five_hour_fallback_without_reset() {
    let home = tempfile::tempdir().unwrap();
    let first = check_and_record_at(home.path(), &AgentKind::ClaudeCode, 1_000, Some(10_000));
    assert!(first.fired);

    let refire = check_and_record_at(home.path(), &AgentKind::ClaudeCode, 10_001, None);
    assert!(refire.fired);
    assert_eq!(refire.expires_at_ms, 10_001 + FIVE_HOUR_WINDOW_MS);
}

#[test]
fn corrupt_mark_file_is_ignored_and_repaired() {
    let home = tempfile::tempdir().unwrap();
    let path = mark_path(home.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{not-json").unwrap();

    let outcome = check_and_record_at(home.path(), &AgentKind::Codex, 1_000, None);
    assert!(outcome.fired);
    assert_eq!(outcome.expires_at_ms, 1_000 + FIVE_HOUR_WINDOW_MS);

    let repaired = std::fs::read_to_string(path).unwrap();
    assert!(repaired.contains("codex"));
}
