use ai_handoff_cli::commands::uninstall::delete_task_argv;
use ai_handoff_core::install::{apply_install, apply_uninstall, targets_for, AgentPresence};

#[test]
fn uninstall_removes_managed_entries_and_preserves_user_keys() {
    let dir = tempfile::tempdir().unwrap();
    let user_home = dir.path();
    std::fs::create_dir_all(user_home.join(".codex")).unwrap();
    std::fs::create_dir_all(user_home.join(".claude")).unwrap();

    std::fs::write(
        user_home.join(".codex/config.toml"),
        "sandbox_mode = \"workspace-write\"\n",
    )
    .unwrap();
    std::fs::write(
        user_home.join(".claude/settings.json"),
        r#"{"model":"opus"}"#,
    )
    .unwrap();

    let ai_home = user_home.join("ai-home");
    let targets = targets_for(
        user_home,
        &ai_home,
        &ai_home.join("ipc"),
        std::path::Path::new("C:/p/ai-handoff.exe"),
    );
    let agents = AgentPresence {
        codex: true,
        claude: true,
    };
    let st = apply_install(&targets, &agents, chrono::Utc::now(), false).unwrap();

    let installed_config = std::fs::read_to_string(user_home.join(".codex/config.toml")).unwrap();
    let mut doc: toml_edit::DocumentMut = installed_config.parse().unwrap();
    doc["sandbox_workspace_write"]["writable_roots"]
        .as_array_mut()
        .unwrap()
        .push("C:/user/root");
    std::fs::write(user_home.join(".codex/config.toml"), doc.to_string()).unwrap();

    apply_uninstall(&targets, &st).unwrap();

    let final_config = std::fs::read_to_string(user_home.join(".codex/config.toml")).unwrap();
    let final_doc: toml_edit::DocumentMut = final_config.parse().unwrap();
    let roots = final_doc["sandbox_workspace_write"]["writable_roots"]
        .as_array()
        .unwrap();
    assert!(roots.iter().any(|v| v.as_str() == Some("C:/user/root")));
    assert!(roots
        .iter()
        .all(|v| !v.as_str().unwrap().contains("ai-home")));
    assert_eq!(final_doc["sandbox_mode"].as_str(), Some("workspace-write"));

    let claude: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(user_home.join(".claude/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(claude["model"], "opus");
    assert!(claude.get("hooks").is_none());
}

#[test]
fn delete_task_argv_matches_expected_schtasks_delete() {
    assert_eq!(
        delete_task_argv(),
        vec!["/Delete", "/TN", "AI Handoff", "/F"]
    );
}
