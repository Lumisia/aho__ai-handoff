use ai_handoff_cli::commands::install::{run_with_targets, scheduled_task_argv};
use ai_handoff_core::install::targets_for;

#[test]
fn install_dry_run_prints_plan_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let user_home = dir.path();
    std::fs::create_dir_all(user_home.join(".codex")).unwrap();
    std::fs::create_dir_all(user_home.join(".claude")).unwrap();

    let codex_config = "sandbox_mode = \"workspace-write\"\n";
    let claude_settings = r#"{"model":"opus"}"#;
    std::fs::write(user_home.join(".codex/config.toml"), codex_config).unwrap();
    std::fs::write(user_home.join(".claude/settings.json"), claude_settings).unwrap();

    let ai_home = user_home.join("ai-home");
    let targets = targets_for(
        user_home,
        &ai_home,
        &ai_home.join("ipc"),
        std::path::Path::new("C:/p/ai-handoff.exe"),
    );

    let mut input: &[u8] = b"";
    let mut output = Vec::new();
    let code = run_with_targets(
        &targets,
        true,
        false,
        None,
        &mut input,
        &mut output,
        false,
        true,
    )
    .unwrap();

    assert_eq!(code, 0);
    let rendered = String::from_utf8(output).unwrap();
    assert!(rendered.contains(".codex"));
    assert!(rendered.contains("config.toml"));
    assert!(rendered.contains("settings.json"));
    assert!(rendered.contains("Dry run only"));
    assert_eq!(
        std::fs::read_to_string(user_home.join(".codex/config.toml")).unwrap(),
        codex_config
    );
    assert_eq!(
        std::fs::read_to_string(user_home.join(".claude/settings.json")).unwrap(),
        claude_settings
    );
    assert!(!user_home.join(".codex/hooks.json").exists());
    assert!(!ai_home.join("install-state.json").exists());
}

#[test]
fn scheduled_task_argv_contains_windows_task_contract() {
    let argv = scheduled_task_argv(
        std::path::Path::new("C:\\p\\ai-handoff-host.exe"),
        std::path::Path::new("C:\\Users\\me\\.ai-handoff"),
    );
    assert!(argv.contains(&"ONLOGON".to_string()));
    assert!(argv.contains(&"AI Handoff".to_string()));
    assert!(argv.contains(
        &"\"C:\\p\\ai-handoff-host.exe\" --home \"C:\\Users\\me\\.ai-handoff\"".to_string()
    ));
}
