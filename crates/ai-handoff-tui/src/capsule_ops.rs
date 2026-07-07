//! Disk mutations for the Capsule tab: toggle the consumption state, edit the
//! summary goal, and delete. Each reads the capsule JSON, applies one minimal
//! change, and writes it back (pretty + atomic via tmp+rename) — matching the
//! daemon's on-disk format so the two never disagree.

use std::path::Path;

use ai_handoff_core::{
    capsule::{Capsule, ConsumptionState},
    capsule_codec::{self, CapsuleCodecError},
    config::CapsuleFormat,
};

#[derive(Debug)]
pub enum CapsuleOpError {
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for CapsuleOpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CapsuleOpError::Io(e) => write!(f, "io error: {e}"),
            CapsuleOpError::Parse(e) => write!(f, "not a valid capsule: {e}"),
        }
    }
}

fn load(path: &Path) -> Result<Capsule, CapsuleOpError> {
    capsule_codec::read_capsule(path).map_err(map_codec_error)
}

fn store(path: &Path, capsule: &Capsule) -> Result<(), CapsuleOpError> {
    capsule_codec::write_capsule(path, capsule, format_for_path(path)).map_err(map_codec_error)
}

fn map_codec_error(err: CapsuleCodecError) -> CapsuleOpError {
    match err {
        CapsuleCodecError::Io(e) => CapsuleOpError::Io(e),
        other => CapsuleOpError::Parse(other.to_string()),
    }
}

fn format_for_path(path: &Path) -> CapsuleFormat {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("md") => CapsuleFormat::Md,
        _ => CapsuleFormat::Json,
    }
}

/// Cycle through the supported manual states. Returns the new snake_case state.
pub fn toggle_state(path: &Path) -> Result<String, CapsuleOpError> {
    let mut capsule = load(path)?;
    let new = capsule.consumption.state.next();
    capsule.consumption.state = new;
    if new == ConsumptionState::Consumed {
        capsule.consumption.consumed_by = Some("ai-handoff-tui".to_string());
        capsule.consumption.consumed_at =
            Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    } else {
        capsule.consumption.consumed_by = None;
        capsule.consumption.consumed_at = None;
    }
    store(path, &capsule)?;
    Ok(new.as_str().to_string())
}

/// A capsule field the user can edit from the detail pane. These are the parts
/// that steer the next agent: the goal, the explicit handoff prompt, and the
/// done / remaining / risks lists. (Ids, timestamps, redaction and the file
/// list are left read-only — editing them would misrepresent what happened.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapField {
    Goal,
    NextPrompt,
    Remaining,
    Done,
    Risks,
}

impl CapField {
    pub fn label(self) -> &'static str {
        match self {
            CapField::Goal => "Goal",
            CapField::NextPrompt => "Next prompt",
            CapField::Remaining => "Remaining",
            CapField::Done => "Done",
            CapField::Risks => "Risks",
        }
    }

    /// List fields are edited as ` | `-separated items on one line.
    pub fn is_list(self) -> bool {
        matches!(self, CapField::Remaining | CapField::Done | CapField::Risks)
    }
}

const LIST_SEP: &str = " | ";

/// The current editable text for `field` (list fields joined by ` | `).
pub fn field_text(capsule: &Capsule, field: CapField) -> String {
    match field {
        CapField::Goal => capsule.summary.goal.clone(),
        CapField::NextPrompt => capsule.next_prompt.clone().unwrap_or_default(),
        CapField::Remaining => capsule.summary.remaining.join(LIST_SEP),
        CapField::Done => capsule.summary.done.join(LIST_SEP),
        CapField::Risks => capsule.summary.risks.join(LIST_SEP),
    }
}

/// Apply edited `text` to `field` and write the capsule back. List fields are
/// split on `|`; an empty next-prompt clears it to `null`.
pub fn set_field(path: &Path, field: CapField, text: &str) -> Result<(), CapsuleOpError> {
    let mut capsule = load(path)?;
    match field {
        CapField::Goal => capsule.summary.goal = text.to_string(),
        CapField::NextPrompt => {
            capsule.next_prompt = if text.trim().is_empty() {
                None
            } else {
                Some(text.to_string())
            };
        }
        CapField::Remaining => capsule.summary.remaining = split_list(text),
        CapField::Done => capsule.summary.done = split_list(text),
        CapField::Risks => capsule.summary.risks = split_list(text),
    }
    store(path, &capsule)
}

fn split_list(text: &str) -> Vec<String> {
    text.split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Delete the capsule file.
pub fn delete(path: &Path) -> Result<(), CapsuleOpError> {
    std::fs::remove_file(path).map_err(CapsuleOpError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_core::capsule::{
        AgentKind, Capsule, Consumption, ConsumptionState, RedactionMeta, Session, Summary,
    };

    fn sample() -> Capsule {
        Capsule {
            schema_version: 2,
            capsule_id: "cap_1".into(),
            project_id: "projX".into(),
            created_at: "2026-06-25T12:00:00Z".into(),
            source_agent: AgentKind::Codex,
            target_agent: AgentKind::ClaudeCode,
            session: Session::default(),
            summary: Summary {
                goal: "ship it".into(),
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
            },
        }
    }

    fn write_sample(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("cap_1.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&sample()).unwrap()).unwrap();
        path
    }

    #[test]
    fn toggle_state_cycles_all_supported_states() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_sample(dir.path());

        assert_eq!(toggle_state(&path).unwrap(), "in_progress");
        let c: Capsule = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(c.consumption.state, ConsumptionState::InProgress);
        assert!(c.consumption.consumed_at.is_none());

        assert_eq!(toggle_state(&path).unwrap(), "blocked");
        assert_eq!(toggle_state(&path).unwrap(), "needs_review");
        assert_eq!(toggle_state(&path).unwrap(), "consumed");
        let c: Capsule = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(c.consumption.state, ConsumptionState::Consumed);
        assert!(c.consumption.consumed_at.is_some());

        assert_eq!(toggle_state(&path).unwrap(), "archived");
        assert_eq!(toggle_state(&path).unwrap(), "pending");
        let c: Capsule = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(c.consumption.state, ConsumptionState::Pending);
        assert!(c.consumption.consumed_at.is_none());
    }

    #[test]
    fn set_field_goal_updates_only_the_goal() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_sample(dir.path());
        set_field(&path, CapField::Goal, "new goal").unwrap();
        let c: Capsule = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(c.summary.goal, "new goal");
        assert_eq!(c.capsule_id, "cap_1"); // untouched
    }

    #[test]
    fn set_field_list_splits_on_pipe_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_sample(dir.path());
        set_field(
            &path,
            CapField::Remaining,
            "wire rotation | add rate limit |  ",
        )
        .unwrap();
        let c: Capsule = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(c.summary.remaining, vec!["wire rotation", "add rate limit"]);
        // and field_text re-joins them for editing
        assert_eq!(
            field_text(&c, CapField::Remaining),
            "wire rotation | add rate limit"
        );
    }

    #[test]
    fn set_field_next_prompt_empty_clears_to_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_sample(dir.path());
        set_field(&path, CapField::NextPrompt, "do the thing").unwrap();
        let c: Capsule = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(c.next_prompt.as_deref(), Some("do the thing"));
        set_field(&path, CapField::NextPrompt, "   ").unwrap();
        let c: Capsule = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(c.next_prompt.is_none());
    }

    #[test]
    fn delete_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_sample(dir.path());
        delete(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn parse_error_on_non_capsule_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{\"nope\":1}").unwrap();
        assert!(matches!(toggle_state(&path), Err(CapsuleOpError::Parse(_))));
    }

    #[test]
    fn md_capsule_edit_and_toggle_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap_1.md");
        ai_handoff_core::capsule_codec::write_capsule(
            &path,
            &sample(),
            ai_handoff_core::config::CapsuleFormat::Md,
        )
        .unwrap();

        set_field(&path, CapField::Goal, "new md goal").unwrap();
        assert_eq!(toggle_state(&path).unwrap(), "in_progress");

        let c = ai_handoff_core::capsule_codec::read_capsule(&path).unwrap();
        assert_eq!(c.summary.goal, "new md goal");
        assert_eq!(c.consumption.state, ConsumptionState::InProgress);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("```ai-handoff-capsule+json"));
    }
}
