use ai_handoff_core::capsule::AgentKind;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const FIVE_HOUR_WINDOW_MS: i64 = 5 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerMarkOutcome {
    pub fired: bool,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct TriggerMark {
    fired_at_ms: i64,
    expires_at_ms: i64,
}

type TriggerMarks = BTreeMap<String, TriggerMark>;

pub fn mark_path(home: &Path) -> PathBuf {
    home.join("store").join("trigger-marks.json")
}

pub fn check_and_record(
    agent: &AgentKind,
    now_ms: i64,
    resets_at_ms: Option<i64>,
) -> TriggerMarkOutcome {
    check_and_record_at(&ai_handoff_core::paths::home(), agent, now_ms, resets_at_ms)
}

pub fn check_and_record_at(
    home: &Path,
    agent: &AgentKind,
    now_ms: i64,
    resets_at_ms: Option<i64>,
) -> TriggerMarkOutcome {
    let path = mark_path(home);
    let mut marks = read_marks(&path).unwrap_or_default();
    let key = agent_key(agent);

    if let Some(existing) = marks.get(key) {
        if now_ms < existing.expires_at_ms {
            return TriggerMarkOutcome {
                fired: false,
                expires_at_ms: existing.expires_at_ms,
            };
        }
    }

    let expires_at_ms = resets_at_ms
        .filter(|reset| *reset > now_ms)
        .unwrap_or_else(|| now_ms.saturating_add(FIVE_HOUR_WINDOW_MS));
    marks.insert(
        key.to_string(),
        TriggerMark {
            fired_at_ms: now_ms,
            expires_at_ms,
        },
    );
    let _ = write_marks(&path, &marks);

    TriggerMarkOutcome {
        fired: true,
        expires_at_ms,
    }
}

fn agent_key(agent: &AgentKind) -> &'static str {
    match agent {
        AgentKind::ClaudeCode => "claude-code",
        AgentKind::Codex => "codex",
    }
}

fn read_marks(path: &Path) -> Option<TriggerMarks> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_marks(path: &Path, marks: &TriggerMarks) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(marks)?;
    std::fs::write(&tmp, json)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(&tmp);
            Err(err)
        }
    }
}
