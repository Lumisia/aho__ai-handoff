use ai_handoff_core::install::{
    apply_install, plan_install, targets_for, AgentPresence, InstallTargets,
};
use chrono::Utc;
use std::path::{Path, PathBuf};

fn fixture() -> (tempfile::TempDir, InstallTargets, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("ai-home");
    let targets = targets_for(
        dir.path(),
        &home,
        &home.join("ipc"),
        Path::new("C:/tools/ai-handoff.exe"),
    );
    let legacy = targets.codex_handoff_skills_dir.join("ai-handoff");
    std::fs::create_dir_all(legacy.join(".claude-plugin")).unwrap();
    std::fs::create_dir_all(legacy.join("skills/handoff")).unwrap();
    std::fs::write(
        legacy.join(".claude-plugin/plugin.json"),
        r#"{"name":"ai-handoff","version":"2.2.4"}"#,
    )
    .unwrap();
    std::fs::write(
        legacy.join("skills/handoff/SKILL.md"),
        "user-modified Codex instructions\n",
    )
    .unwrap();
    std::fs::write(legacy.join("personal-notes.txt"), "keep these notes\n").unwrap();
    (dir, targets, legacy)
}

fn agents() -> AgentPresence {
    AgentPresence {
        codex: true,
        claude: true,
    }
}

#[test]
fn plugin_install_archives_legacy_codex_bundle_intact_and_preserves_claude() {
    let (_dir, targets, legacy) = fixture();
    apply_install(&targets, &agents(), Utc::now(), true).unwrap();
    assert!(
        !legacy.exists(),
        "obsolete nested Codex skills are still discoverable"
    );
    let backup_root = targets
        .codex_handoff_skills_dir
        .parent()
        .unwrap()
        .join("legacy-skill-backups");
    let backups: Vec<_> = std::fs::read_dir(&backup_root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        std::fs::read_to_string(backups[0].join("ai-handoff/skills/handoff/SKILL.md")).unwrap(),
        "user-modified Codex instructions\n"
    );
    assert_eq!(
        std::fs::read_to_string(backups[0].join("ai-handoff/personal-notes.txt")).unwrap(),
        "keep these notes\n"
    );
    assert!(targets
        .claude_plugin_dir
        .join("skills/handoff/SKILL.md")
        .is_file());
    assert!(targets
        .codex_plugin_dir
        .join("skills/handoff/SKILL.md")
        .is_file());
    apply_install(&targets, &agents(), Utc::now(), true).unwrap();
    assert_eq!(
        std::fs::read_dir(backup_root).unwrap().count(),
        1,
        "reinstall must not create another archive"
    );
}

#[test]
fn plugin_plan_warns_about_legacy_codex_bundle_without_writes() {
    let (_dir, targets, legacy) = fixture();
    let plan = plan_install(&targets, &agents(), Utc::now(), true).unwrap();
    assert!(
        plan.duplicates
            .iter()
            .any(|finding| finding.agent == "codex"
                && finding.detail.contains("legacy-skill-backups")),
        "plan omitted legacy skill migration"
    );
    assert!(legacy.join("personal-notes.txt").exists());
    assert!(!targets
        .codex_handoff_skills_dir
        .parent()
        .unwrap()
        .join("legacy-skill-backups")
        .exists());
    assert!(!targets.codex_plugin_dir.exists());
}

#[test]
fn no_plugin_install_leaves_nested_codex_bundle_untouched() {
    let (_dir, targets, legacy) = fixture();
    apply_install(&targets, &agents(), Utc::now(), false).unwrap();
    assert_eq!(
        std::fs::read_to_string(legacy.join("skills/handoff/SKILL.md")).unwrap(),
        "user-modified Codex instructions\n"
    );
    assert!(!targets.codex_plugin_dir.exists());
}

#[test]
fn failed_canonical_bundle_generation_keeps_legacy_codex_bundle() {
    let (_dir, targets, legacy) = fixture();
    std::fs::create_dir_all(targets.codex_plugin_dir.parent().unwrap()).unwrap();
    std::fs::write(&targets.codex_plugin_dir, "blocks directory creation").unwrap();
    assert!(apply_install(&targets, &agents(), Utc::now(), true).is_err());
    assert!(legacy.join("skills/handoff/SKILL.md").exists());
}

#[test]
fn foreign_and_malformed_bundles_are_preserved() {
    for manifest in [r#"{"name":"other-plugin"}"#, "{broken", "null"] {
        let (_dir, targets, legacy) = fixture();
        std::fs::write(legacy.join(".claude-plugin/plugin.json"), manifest).unwrap();
        apply_install(&targets, &agents(), Utc::now(), true).unwrap();
        assert!(legacy.join("personal-notes.txt").is_file());
        assert!(!targets
            .codex_handoff_skills_dir
            .parent()
            .unwrap()
            .join("legacy-skill-backups")
            .exists());
    }
}

#[test]
fn claude_only_install_does_not_migrate_codex_skills() {
    let (_dir, targets, legacy) = fixture();
    apply_install(
        &targets,
        &AgentPresence {
            codex: false,
            claude: true,
        },
        Utc::now(),
        true,
    )
    .unwrap();
    assert!(legacy.join("personal-notes.txt").is_file());
    assert!(targets
        .claude_plugin_dir
        .join("skills/handoff/SKILL.md")
        .is_file());
}

#[test]
fn manifest_without_known_nested_skills_is_preserved() {
    let (_dir, targets, legacy) = fixture();
    std::fs::remove_file(legacy.join("skills/handoff/SKILL.md")).unwrap();
    apply_install(&targets, &agents(), Utc::now(), true).unwrap();
    assert!(legacy.join("personal-notes.txt").is_file());
}

#[test]
fn blocked_archive_destination_aborts_before_installing_plugin_or_config() {
    let (_dir, targets, legacy) = fixture();
    let archive = targets
        .codex_handoff_skills_dir
        .parent()
        .unwrap()
        .join("legacy-skill-backups");
    std::fs::write(&archive, "user-owned file\n").unwrap();

    assert!(plan_install(&targets, &agents(), Utc::now(), true).is_err());
    assert!(apply_install(&targets, &agents(), Utc::now(), true).is_err());
    assert!(legacy.join("personal-notes.txt").is_file());
    assert!(!targets.codex_plugin_dir.exists());
    assert!(!targets.codex_config.exists());
    assert!(!targets.agents_marketplace.exists());
    assert!(!targets.claude_settings.exists());
    assert_eq!(
        std::fs::read_to_string(archive).unwrap(),
        "user-owned file\n"
    );
}

#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) {
    // A directory junction needs no developer mode or elevation.
    let result = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "junction fixture failed: {:?}",
        result
    );
}

#[test]
fn linked_legacy_bundle_is_not_followed_or_moved() {
    let (dir, targets, legacy) = fixture();
    let outside = dir.path().join("outside-bundle");
    std::fs::rename(&legacy, &outside).unwrap();
    link_dir(&outside, &legacy);
    apply_install(&targets, &agents(), Utc::now(), true).unwrap();
    assert!(std::fs::symlink_metadata(&legacy).is_ok());
    assert_eq!(
        std::fs::read_to_string(outside.join("personal-notes.txt")).unwrap(),
        "keep these notes\n"
    );
    #[cfg(windows)]
    std::fs::remove_dir(&legacy).unwrap();
}

#[test]
fn linked_plain_user_skill_is_preserved_even_when_content_matches() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("skills");
    let outside = dir.path().join("outside-skill");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let body = include_str!("../../../skills/handoff-config/SKILL.md");
    std::fs::write(outside.join("SKILL.md"), body).unwrap();
    let skill = root.join("handoff-config");
    link_dir(&outside, &skill);

    ai_handoff_core::install::remove_handoff_user_skills(&root).unwrap();

    assert!(std::fs::symlink_metadata(&skill).is_ok());
    assert_eq!(
        std::fs::read_to_string(outside.join("SKILL.md")).unwrap(),
        body
    );
    #[cfg(windows)]
    std::fs::remove_dir(&skill).unwrap();
}

#[test]
fn linked_archive_directory_cannot_redirect_migration() {
    let (dir, targets, legacy) = fixture();
    let outside = dir.path().join("outside-archive");
    std::fs::create_dir(&outside).unwrap();
    let archive = targets
        .codex_handoff_skills_dir
        .parent()
        .unwrap()
        .join("legacy-skill-backups");
    link_dir(&outside, &archive);
    assert!(apply_install(&targets, &agents(), Utc::now(), true).is_err());
    assert!(legacy.join("personal-notes.txt").is_file());
    assert!(!targets.codex_plugin_dir.exists());
    assert!(!targets.codex_config.exists());
    assert!(!targets.agents_marketplace.exists());
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    #[cfg(windows)]
    std::fs::remove_dir(&archive).unwrap();
}
