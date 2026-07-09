use ai_handoff_core::{
    capsule::{canonical_agent_id, Capsule, Consumption, ConsumptionState},
    capsule_codec, config, paths,
};
use chrono::{DateTime, Utc};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

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

/// Distinguish "no capsules yet" (missing project dir — fine) from a store
/// the daemon cannot read (sandbox/permissions). Without this check the
/// silent-empty `list_pending` disguises an unreadable store as "nothing
/// pending", and a consume answers `{}` as if it succeeded at finding nothing.
pub fn store_readable(project_id: &str) -> std::io::Result<()> {
    match std::fs::read_dir(paths::project_dir(project_id)) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
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

/// Claim a pending capsule. Fails with `InvalidInput` when the capsule is no
/// longer pending — the re-check runs inside the capsule lock, so of two
/// racing consumers exactly one wins and the loser gets an error instead of
/// silently overwriting `consumed_by`.
pub fn mark_consumed(
    project_id: &str,
    capsule_id: &str,
    by: &str,
    now: DateTime<Utc>,
    despite_target: bool,
) -> std::io::Result<()> {
    let mut was_pending = true;
    update_capsule(project_id, capsule_id, |capsule| {
        was_pending = capsule.consumption.state == ConsumptionState::Pending;
        if !was_pending {
            return;
        }
        capsule.consumption = Consumption {
            state: ConsumptionState::Consumed,
            consumed_by: Some(by.to_string()),
            consumed_at: Some(now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            consumed_despite_target: despite_target,
        };
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
    let _lock = CapsuleLock::acquire(&path)?;
    let mut capsule = capsule_codec::read_capsule(&path).map_err(std::io::Error::other)?;
    mutate(&mut capsule);
    let format = if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
        config::CapsuleFormat::Md
    } else {
        config::CapsuleFormat::Json
    };
    capsule_codec::write_capsule(&path, &capsule, format).map_err(std::io::Error::other)
}

/// Advisory per-capsule lock file guarding the read-modify-write in
/// [`update_capsule`] against a concurrent writer (second daemon instance or
/// an external process). `create_new` is atomic on both Windows and Unix. A
/// lock left behind by a crashed process is stolen once it is older than
/// [`Self::STALE`].
struct CapsuleLock(PathBuf);

impl CapsuleLock {
    const RETRY_FOR: Duration = Duration::from_millis(500);
    const RETRY_EVERY: Duration = Duration::from_millis(25);
    const STALE: Duration = Duration::from_secs(5);

    fn acquire(capsule_path: &Path) -> std::io::Result<Self> {
        // `.lock` never collides with capsule extensions (json/md), and both
        // formats of the same id share one lock — which is exactly right.
        let lock_path = capsule_path.with_extension("lock");
        let deadline = Instant::now() + Self::RETRY_FOR;
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(_) => return Ok(Self(lock_path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&lock_path)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|modified| modified.elapsed().ok())
                        .is_some_and(|age| age > Self::STALE);
                    if stale {
                        let _ = std::fs::remove_file(&lock_path);
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::WouldBlock,
                            "capsule is locked by another process",
                        ));
                    }
                    std::thread::sleep(Self::RETRY_EVERY);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for CapsuleLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn is_capsule_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("json" | "md")
    )
}

/// A capsule id must be a bare file stem. Anything with path separators,
/// parent segments, or a drive/root prefix could escape the project directory
/// when joined (e.g. a retarget request naming `../<other-project>/cap_x`).
fn is_safe_capsule_id(capsule_id: &str) -> bool {
    let mut components = Path::new(capsule_id).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(only)), None) if only == std::ffi::OsStr::new(capsule_id)
    )
}

fn find_capsule_path(project_id: &str, capsule_id: &str) -> Option<PathBuf> {
    if !is_safe_capsule_id(capsule_id) {
        return None;
    }
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
    fn store_readable_treats_missing_project_dir_as_ok() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        // No capsules ever saved: not an error, just nothing pending.
        store_readable("proj-without-capsules").unwrap();

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn capsule_id_with_path_segments_is_rejected() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        // Victim capsule in ANOTHER project's bucket.
        let mut victim = capsule("cap-victim", "2026-06-25T12:00:00Z", Some("codex"));
        victim.project_id = "projY".into();
        let victim_path = save_capsule(&victim).unwrap();

        // A traversal id from projX must not reach projY's file.
        let evil = "../projY/cap-victim";
        assert_eq!(
            retarget("projX", evil, Some("grok".to_string()))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(
            mark_consumed(
                "projX",
                evil,
                "grok",
                chrono::Utc.with_ymd_and_hms(2026, 6, 25, 14, 0, 0).unwrap(),
                false,
            )
            .unwrap_err()
            .kind(),
            std::io::ErrorKind::NotFound
        );

        let untouched = ai_handoff_core::capsule_codec::read_capsule(&victim_path).unwrap();
        assert_eq!(untouched.target_agent.as_deref(), Some("codex"));
        assert_eq!(untouched.consumption.state, ConsumptionState::Pending);

        assert!(is_safe_capsule_id("cap_20260625_120000_abcd"));
        assert!(!is_safe_capsule_id("../x"));
        assert!(!is_safe_capsule_id("a/b"));
        assert!(!is_safe_capsule_id("a\\b"));
        assert!(!is_safe_capsule_id(".."));
        assert!(!is_safe_capsule_id(""));

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn second_consume_of_same_capsule_fails() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let path = save_capsule(&capsule("cap", "2026-06-25T12:00:00Z", None)).unwrap();
        let now = chrono::Utc.with_ymd_and_hms(2026, 6, 25, 14, 0, 0).unwrap();
        mark_consumed("projX", "cap", "claude-code", now, false).unwrap();

        // The losing racer gets InvalidInput and the first claim survives.
        assert_eq!(
            mark_consumed("projX", "cap", "codex", now, false)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        let updated = ai_handoff_core::capsule_codec::read_capsule(&path).unwrap();
        assert_eq!(
            updated.consumption.consumed_by.as_deref(),
            Some("claude-code")
        );

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
