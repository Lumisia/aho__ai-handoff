use ai_handoff_core::{
    capsule::{AgentKind, Capsule, Consumption, ConsumptionState},
    capsule_codec, config, paths,
};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn save_capsule(c: &Capsule) -> std::io::Result<PathBuf> {
    let format = config::load().capsule.format;
    let path =
        capsule_codec::capsule_path(&paths::project_dir(&c.project_id), &c.capsule_id, format);
    capsule_codec::write_capsule(&path, c, format).map_err(std::io::Error::other)?;
    Ok(path)
}

pub fn save_project_label(project_id: &str, cwd: &Path) -> std::io::Result<()> {
    let Some(label) = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return Ok(());
    };
    let dir = paths::project_dir(project_id);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("project.label"), label)
}

pub fn find_pending(project_id: &str) -> Option<Capsule> {
    let dir = paths::project_dir(project_id);
    let entries = std::fs::read_dir(dir).ok()?;

    entries
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
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, _, capsule)| capsule)
}

pub fn mark_consumed(
    project_id: &str,
    capsule_id: &str,
    by: AgentKind,
    now: DateTime<Utc>,
) -> std::io::Result<()> {
    let path = find_capsule_path(project_id, capsule_id)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "capsule not found"))?;
    let mut capsule = capsule_codec::read_capsule(&path).map_err(std::io::Error::other)?;
    capsule.consumption = Consumption {
        state: ConsumptionState::Consumed,
        consumed_by: Some(format_agent(by)),
        consumed_at: Some(now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    };
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

fn format_agent(agent: AgentKind) -> String {
    match agent {
        AgentKind::ClaudeCode => "claude-code".to_string(),
        AgentKind::Codex => "codex".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use ai_handoff_core::capsule::{
        AgentKind, Capsule, Consumption, ConsumptionState, RedactionMeta, Session, Summary,
    };
    use chrono::TimeZone;

    fn capsule(id: &str, created_at: &str) -> Capsule {
        Capsule {
            schema_version: 2,
            capsule_id: id.into(),
            project_id: "projX".into(),
            created_at: created_at.into(),
            source_agent: AgentKind::Codex,
            target_agent: AgentKind::ClaudeCode,
            session: Session::default(),
            summary: Summary {
                goal: id.into(),
                done: vec![],
                remaining: vec![],
                risks: vec![],
            },
            files: vec![],
            next_prompt: None,
            redaction: RedactionMeta {
                applied: true,
                ruleset: "default-v2".into(),
            },
            consumption: Consumption {
                state: ConsumptionState::Pending,
                consumed_by: None,
                consumed_at: None,
            },
        }
    }

    #[test]
    fn save_find_pending_and_mark_consumed() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        save_capsule(&capsule("old", "2026-06-25T12:00:00Z")).unwrap();
        save_capsule(&capsule("new", "2026-06-25T13:00:00Z")).unwrap();

        let pending = find_pending("projX").unwrap();
        assert_eq!(pending.capsule_id, "new");

        mark_consumed(
            "projX",
            "new",
            AgentKind::ClaudeCode,
            chrono::Utc.with_ymd_and_hms(2026, 6, 25, 14, 0, 0).unwrap(),
        )
        .unwrap();

        let pending = find_pending("projX").unwrap();
        assert_eq!(pending.capsule_id, "old");

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

        let path = save_capsule(&capsule("md-new", "2026-06-25T13:00:00Z")).unwrap();
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("md"));
        assert_eq!(find_pending("projX").unwrap().capsule_id, "md-new");

        mark_consumed(
            "projX",
            "md-new",
            AgentKind::ClaudeCode,
            chrono::Utc.with_ymd_and_hms(2026, 6, 25, 14, 0, 0).unwrap(),
        )
        .unwrap();

        let updated = ai_handoff_core::capsule_codec::read_capsule(&path).unwrap();
        assert_eq!(updated.consumption.state, ConsumptionState::Consumed);

        std::env::remove_var("AI_HANDOFF_HOME");
    }
}
