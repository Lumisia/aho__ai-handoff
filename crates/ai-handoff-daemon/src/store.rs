use ai_handoff_core::{
    capsule::{canonical_agent_id, Capsule, Consumption, ConsumptionState},
    capsule_codec, config, paths,
};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn save_capsule(c: &Capsule) -> std::io::Result<PathBuf> {
    let format = config::load().capsule.format;
    ai_handoff_core::secure_fs::ensure_private_dir(&paths::store_dir())?;
    let path =
        capsule_codec::capsule_path(&paths::project_dir(&c.project_id), &c.capsule_id, format);
    capsule_codec::write_capsule(&path, c, format).map_err(std::io::Error::other)?;
    Ok(path)
}

pub fn save_project_label(project_id: &str, cwd: &Path) -> std::io::Result<()> {
    // A linked worktree's basename is an auto-generated session name (e.g.
    // Claude Code's `loving-sanderson-9a5e09`); label from the primary
    // checkout so the shared capsule bucket keeps the real project name.
    let main_root = ai_handoff_core::fingerprint::linked_worktree_main_root(cwd);
    let Some(label) = main_root
        .as_deref()
        .unwrap_or(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return Ok(());
    };
    let dir = paths::project_dir(project_id);
    ai_handoff_core::secure_fs::ensure_private_dir(&paths::store_dir())?;
    ai_handoff_core::secure_fs::ensure_private_dir(&dir)?;
    ai_handoff_core::secure_fs::write_private_file(&dir.join("project.label"), label.as_bytes())
}

/// Every pending capsule of the project, newest first (creation time, then
/// file mtime as tiebreaker). Unreadable files are skipped.
pub fn list_pending(project_id: &str) -> Vec<Capsule> {
    let dir = paths::project_dir(project_id);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut pending: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !is_capsule_file(&path) {
                return None;
            }

            let capsule = capsule_codec::read_capsule(&path).ok()?;
            if capsule.consumption.state != ConsumptionState::Pending {
                return None;
            }

            let created = parse_created_at(&capsule.created_at);
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((created, modified, capsule))
        })
        .collect();
    pending.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    pending.into_iter().map(|(_, _, capsule)| capsule).collect()
}

/// Pending capsules this agent may auto-claim: addressed to it or open
/// (no target). Capsules targeting a different agent stay out of this list —
/// they remain visible via [`list_pending`] and reachable via retarget/force.
pub fn pending_for(project_id: &str, agent: &str) -> Vec<Capsule> {
    // Canonicalize both sides: capsules written by hand or by external tools
    // may carry alias/case/whitespace variants of an agent id.
    let agent = canonical_agent_id(agent);
    list_pending(project_id)
        .into_iter()
        .filter(|capsule| match &capsule.target_agent {
            Some(target) => canonical_agent_id(target) == agent,
            None => true,
        })
        .collect()
}

/// The newest pending capsule this agent may auto-claim.
pub fn find_pending(project_id: &str, agent: &str) -> Option<Capsule> {
    pending_for(project_id, agent).into_iter().next()
}

pub fn mark_consumed(
    project_id: &str,
    capsule_id: &str,
    by: &str,
    now: DateTime<Utc>,
    despite_target: bool,
) -> std::io::Result<()> {
    update_capsule(project_id, capsule_id, |capsule| {
        capsule.consumption = Consumption {
            state: ConsumptionState::Consumed,
            consumed_by: Some(by.to_string()),
            consumed_at: Some(now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            consumed_despite_target: despite_target,
        };
    })
}

/// Point a pending capsule at a different agent, or open it up (`None`).
/// Fails when the capsule does not exist or is no longer pending.
pub fn retarget(
    project_id: &str,
    capsule_id: &str,
    new_target: Option<String>,
) -> std::io::Result<()> {
    let mut was_pending = true;
    update_capsule(project_id, capsule_id, |capsule| {
        was_pending = capsule.consumption.state == ConsumptionState::Pending;
        if was_pending {
            capsule.target_agent = new_target.clone();
        }
    })?;
    if was_pending {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "capsule is not pending",
        ))
    }
}

fn update_capsule(
    project_id: &str,
    capsule_id: &str,
    mutate: impl FnOnce(&mut Capsule),
) -> std::io::Result<()> {
    let path = find_capsule_path(project_id, capsule_id)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "capsule not found"))?;
    let mut capsule = capsule_codec::read_capsule(&path).map_err(std::io::Error::other)?;
    mutate(&mut capsule);
    let format = if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
        config::CapsuleFormat::Md
    } else {
        config::CapsuleFormat::Json
    };
    capsule_codec::write_capsule(&path, &capsule, format).map_err(std::io::Error::other)
}

fn is_capsule_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("json" | "md")
    )
}

fn find_capsule_path(project_id: &str, capsule_id: &str) -> Option<PathBuf> {
    let dir = paths::project_dir(project_id);
    [config::CapsuleFormat::Json, config::CapsuleFormat::Md]
        .into_iter()
        .map(|format| capsule_codec::capsule_path(&dir, capsule_id, format))
        .find(|path| path.exists())
}

fn parse_created_at(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use ai_handoff_core::capsule::{
        Capsule, Consumption, ConsumptionState, RedactionMeta, Session, Summary,
    };
    use chrono::TimeZone;

    fn capsule(id: &str, created_at: &str, target: Option<&str>) -> Capsule {
        Capsule {
            schema_version: 2,
            capsule_id: id.into(),
            project_id: "projX".into(),
            created_at: created_at.into(),
            source_agent: "codex".into(),
            target_agent: target.map(str::to_string),
            session: Session::default(),
            summary: Summary {
                goal: id.into(),
                done: vec![],
                remaining: vec![],
                risks: vec![],
            },
            files: vec![],
            next_prompt: None,
            workspace: None,
            redaction: RedactionMeta {
                applied: true,
                ruleset: "default-v2".into(),
            },
            consumption: Consumption {
                state: ConsumptionState::Pending,
                consumed_by: None,
                consumed_at: None,
                consumed_despite_target: false,
            },
        }
    }

    #[test]
    fn project_label_uses_main_checkout_name_for_linked_worktree() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        // Main checkout plus a linked worktree whose basename is an
        // auto-generated session name.
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("myproject");
        let wt_meta = main.join(".git").join("worktrees").join("wt");
        std::fs::create_dir_all(&wt_meta).unwrap();
        std::fs::write(wt_meta.join("commondir"), "../..\n").unwrap();
        let worktree = tmp.path().join("loving-sanderson-9a5e09");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", wt_meta.to_string_lossy()),
        )
        .unwrap();

        save_project_label("projX", &worktree).unwrap();
        let label =
            std::fs::read_to_string(paths::project_dir("projX").join("project.label")).unwrap();
        assert_eq!(label, "myproject");

        // Non-repo cwd keeps the old basename fallback.
        let plain = tmp.path().join("plain-dir");
        std::fs::create_dir_all(&plain).unwrap();
        save_project_label("projY", &plain).unwrap();
        let label =
            std::fs::read_to_string(paths::project_dir("projY").join("project.label")).unwrap();
        assert_eq!(label, "plain-dir");

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn save_find_pending_and_mark_consumed() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        save_capsule(&capsule("old", "2026-06-25T12:00:00Z", Some("claude-code"))).unwrap();
        save_capsule(&capsule("new", "2026-06-25T13:00:00Z", Some("claude-code"))).unwrap();

        let pending = find_pending("projX", "claude-code").unwrap();
        assert_eq!(pending.capsule_id, "new");

        mark_consumed(
            "projX",
            "new",
            "claude-code",
            chrono::Utc.with_ymd_and_hms(2026, 6, 25, 14, 0, 0).unwrap(),
            false,
        )
        .unwrap();

        let pending = find_pending("projX", "claude-code").unwrap();
        assert_eq!(pending.capsule_id, "old");

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn find_pending_filters_by_target_before_recency() {
        // The old shadowing bug: a newer capsule for the OTHER agent must not
        // hide an older capsule addressed to me.
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        save_capsule(&capsule(
            "for-claude",
            "2026-06-25T12:00:00Z",
            Some("claude-code"),
        ))
        .unwrap();
        save_capsule(&capsule("for-codex", "2026-06-25T13:00:00Z", Some("codex"))).unwrap();

        assert_eq!(
            find_pending("projX", "claude-code").unwrap().capsule_id,
            "for-claude"
        );
        assert_eq!(
            find_pending("projX", "codex").unwrap().capsule_id,
            "for-codex"
        );

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn find_pending_canonicalizes_target_comparison() {
        // Hand-edited or externally written capsules may carry "Gemini",
        // "claude_code", trailing spaces, etc. — matching must not be raw
        // string equality.
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        save_capsule(&capsule("messy", "2026-06-25T12:00:00Z", Some("Gemini "))).unwrap();
        save_capsule(&capsule(
            "alias",
            "2026-06-25T13:00:00Z",
            Some("claude_code"),
        ))
        .unwrap();

        assert_eq!(find_pending("projX", "gemini").unwrap().capsule_id, "messy");
        assert_eq!(
            find_pending("projX", "claude-code").unwrap().capsule_id,
            "alias"
        );

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn open_capsule_is_consumable_by_any_agent() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        save_capsule(&capsule("open", "2026-06-25T12:00:00Z", None)).unwrap();

        // Grok never appears in any enum — open capsules still reach it.
        assert_eq!(find_pending("projX", "grok").unwrap().capsule_id, "open");
        assert_eq!(
            find_pending("projX", "claude-code").unwrap().capsule_id,
            "open"
        );

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn other_target_capsule_is_not_auto_claimed_but_stays_visible() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        save_capsule(&capsule("for-codex", "2026-06-25T12:00:00Z", Some("codex"))).unwrap();

        assert!(find_pending("projX", "grok").is_none());
        let all = list_pending("projX");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].capsule_id, "for-codex");

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn pending_for_returns_mine_and_open_newest_first() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        save_capsule(&capsule("mine-old", "2026-06-25T11:00:00Z", Some("grok"))).unwrap();
        save_capsule(&capsule("open-mid", "2026-06-25T12:00:00Z", None)).unwrap();
        save_capsule(&capsule("theirs", "2026-06-25T13:00:00Z", Some("codex"))).unwrap();

        let mine = pending_for("projX", "grok");
        let ids: Vec<&str> = mine.iter().map(|c| c.capsule_id.as_str()).collect();
        assert_eq!(ids, vec!["open-mid", "mine-old"]);

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn retarget_redirects_or_opens_a_pending_capsule() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        save_capsule(&capsule("cap", "2026-06-25T12:00:00Z", Some("codex"))).unwrap();
        assert!(find_pending("projX", "grok").is_none());

        retarget("projX", "cap", Some("grok".to_string())).unwrap();
        assert_eq!(find_pending("projX", "grok").unwrap().capsule_id, "cap");
        assert!(find_pending("projX", "codex").is_none());

        retarget("projX", "cap", None).unwrap();
        assert_eq!(find_pending("projX", "codex").unwrap().capsule_id, "cap");

        assert!(retarget("projX", "missing", None).is_err());

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn forced_consume_records_despite_target() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let path = save_capsule(&capsule("cap", "2026-06-25T12:00:00Z", Some("codex"))).unwrap();
        mark_consumed(
            "projX",
            "cap",
            "grok",
            chrono::Utc.with_ymd_and_hms(2026, 6, 25, 14, 0, 0).unwrap(),
            true,
        )
        .unwrap();

        let updated = ai_handoff_core::capsule_codec::read_capsule(&path).unwrap();
        assert_eq!(updated.consumption.state, ConsumptionState::Consumed);
        assert_eq!(updated.consumption.consumed_by.as_deref(), Some("grok"));
        assert!(updated.consumption.consumed_despite_target);

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn md_format_save_find_pending_and_mark_consumed() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[capsule]\nformat = \"md\"\n",
        )
        .unwrap();

        let path = save_capsule(&capsule(
            "md-new",
            "2026-06-25T13:00:00Z",
            Some("claude-code"),
        ))
        .unwrap();
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("md"));
        assert_eq!(
            find_pending("projX", "claude-code").unwrap().capsule_id,
            "md-new"
        );

        mark_consumed(
            "projX",
            "md-new",
            "claude-code",
            chrono::Utc.with_ymd_and_hms(2026, 6, 25, 14, 0, 0).unwrap(),
            false,
        )
        .unwrap();

        let updated = ai_handoff_core::capsule_codec::read_capsule(&path).unwrap();
        assert_eq!(updated.consumption.state, ConsumptionState::Consumed);

        std::env::remove_var("AI_HANDOFF_HOME");
    }
}
