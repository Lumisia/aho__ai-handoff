//! Capsule structs, ID generation, integrity hashing, and validation for v2.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Sub-structs referenced by Capsule
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Session {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub turn_count: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Summary {
    pub goal: String,
    pub done: Vec<String>,
    pub remaining: Vec<String>,
    pub risks: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileChange {
    pub path: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RedactionMeta {
    pub applied: bool,
    pub ruleset: String,
}

/// Git state of the workspace at capsule-creation time. Attached automatically
/// by the daemon (never trusted from the agent payload) so the consuming side
/// can detect drift between the capsule and the current checkout. All fields
/// optional: absent when the project is not a git repo.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Workspace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty_files: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Consumption {
    pub state: ConsumptionState,
    #[serde(default)]
    pub consumed_by: Option<String>,
    #[serde(default)]
    pub consumed_at: Option<String>,
    /// True when a `--force` consume took a capsule that targeted a different
    /// agent — kept for auditability of overridden routing.
    #[serde(default, skip_serializing_if = "is_false")]
    pub consumed_despite_target: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// The agent kind — serialized as kebab-case strings. Used for the agents
/// with local usage/trigger integrations (hooks, statusline, accounts).
/// Capsule routing uses plain agent-id strings instead, so unknown agents
/// (Cursor, Gemini, Grok, ...) can hand off without touching this enum.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
}

impl AgentKind {
    /// The canonical agent-id string used in capsule fields.
    pub fn as_canonical_str(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::Codex => "codex",
        }
    }
}

/// Normalize a user- or agent-supplied agent id to its canonical form:
/// lowercase, kebab-case, known aliases folded ("claude" → "claude-code").
/// Unknown ids pass through normalized (forward compatibility); empty input
/// returns `None`.
pub fn canonical_agent_id(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    if normalized.is_empty() {
        return None;
    }
    Some(match normalized.as_str() {
        "claude" | "claude-code" | "claudecode" => "claude-code".to_string(),
        _ => normalized,
    })
}

/// Consumption state — serialized as snake_case strings.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsumptionState {
    Pending,
    InProgress,
    Blocked,
    NeedsReview,
    Consumed,
    Archived,
}

impl ConsumptionState {
    pub fn as_str(self) -> &'static str {
        match self {
            ConsumptionState::Pending => "pending",
            ConsumptionState::InProgress => "in_progress",
            ConsumptionState::Blocked => "blocked",
            ConsumptionState::NeedsReview => "needs_review",
            ConsumptionState::Consumed => "consumed",
            ConsumptionState::Archived => "archived",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ConsumptionState::Pending => ConsumptionState::InProgress,
            ConsumptionState::InProgress => ConsumptionState::Blocked,
            ConsumptionState::Blocked => ConsumptionState::NeedsReview,
            ConsumptionState::NeedsReview => ConsumptionState::Consumed,
            ConsumptionState::Consumed => ConsumptionState::Archived,
            ConsumptionState::Archived => ConsumptionState::Pending,
        }
    }
}

// ---------------------------------------------------------------------------
// Capsule — the top-level v2 handoff document
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Capsule {
    pub schema_version: u32,
    pub capsule_id: String,
    pub project_id: String,
    pub created_at: String,
    /// Canonical agent-id string of the capsule's author (e.g. "claude-code").
    pub source_agent: String,
    /// Preferred consumer as a routing hint. `None` means open — any agent may
    /// consume. Serialized as an explicit null so older readers still see the
    /// key. A `Some` value never hard-locks the capsule: other agents can
    /// still see it (peek/list) and take it via retarget or a forced consume.
    #[serde(default)]
    pub target_agent: Option<String>,
    pub session: Session,
    pub summary: Summary,
    pub files: Vec<FileChange>,
    #[serde(default)]
    pub next_prompt: Option<String>,
    /// Auto-collected git snapshot (schema v2 stays compatible: optional and
    /// omitted when absent, so old capsules and old readers both keep working).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Workspace>,
    pub redaction: RedactionMeta,
    pub consumption: Consumption,
}

// ---------------------------------------------------------------------------
// ID generation
// ---------------------------------------------------------------------------

/// Generate a stable capsule ID: `cap_YYYYMMDD_HHMMSS_<4hex>`.
///
/// The 4 hex chars come from the first 2 bytes of a fresh `uuid::Uuid::new_v4()`.
pub fn new_capsule_id(now: DateTime<Utc>) -> String {
    let ts = now.format("%Y%m%d_%H%M%S");
    let uuid = uuid::Uuid::new_v4();
    let bytes = uuid.as_bytes();
    let suffix = format!("{:02x}{:02x}", bytes[0], bytes[1]);
    format!("cap_{ts}_{suffix}")
}

// ---------------------------------------------------------------------------
// Integrity / hashing
// ---------------------------------------------------------------------------

/// Return `sha256:<hex>` over the canonical JSON of the capsule.
///
/// Canonical = BTreeMap-ordered keys (serde_json default) + compact (no
/// whitespace). The `"integrity"` key is removed before hashing so this is
/// forward-compatible with capsules that carry a self-referencing integrity
/// field.
pub fn payload_sha256(c: &Capsule) -> String {
    // Serialize to a Value; the default serde_json Map is a BTreeMap (sorted).
    let mut v: serde_json::Value = serde_json::to_value(c).expect("Capsule is always serializable");
    // Remove "integrity" if present (forward-compat).
    if let Some(obj) = v.as_object_mut() {
        obj.remove("integrity");
    }
    let canonical = serde_json::to_string(&v).expect("Value is always serializable");
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate a capsule against v2 schema rules.
///
/// Returns `Ok(())` when all checks pass, or `Err(reasons)` listing each
/// failure.
pub fn validate(c: &Capsule) -> Result<(), Vec<String>> {
    let mut reasons: Vec<String> = Vec::new();

    if c.schema_version != 2 {
        reasons.push(format!(
            "schema_version must be 2, got {}",
            c.schema_version
        ));
    }
    if c.capsule_id.is_empty() {
        reasons.push("capsule_id must not be empty".into());
    }
    if c.project_id.is_empty() {
        reasons.push("project_id must not be empty".into());
    }
    if c.source_agent.trim().is_empty() {
        reasons.push("source_agent must not be empty".into());
    }
    if let Some(target) = &c.target_agent {
        if target.trim().is_empty() {
            reasons.push("target_agent must be a non-empty agent id or null".into());
        }
    }

    if reasons.is_empty() {
        Ok(())
    } else {
        Err(reasons)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample() -> Capsule {
        Capsule {
            schema_version: 2,
            capsule_id: "cap_20260625_123456_abcd".into(),
            project_id: "projX".into(),
            created_at: "2026-06-25T12:34:56Z".into(),
            source_agent: "codex".into(),
            target_agent: Some("claude-code".into()),
            session: Session::default(),
            summary: Summary {
                goal: "g".into(),
                done: vec![],
                remaining: vec![],
                risks: vec![],
            },
            files: vec![],
            next_prompt: Some("do x".into()),
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
    fn id_format() {
        let dt = chrono::Utc
            .with_ymd_and_hms(2026, 6, 25, 12, 34, 56)
            .unwrap();
        let id = new_capsule_id(dt);
        assert!(id.starts_with("cap_20260625_123456_"));
        assert_eq!(id.len(), "cap_20260625_123456_".len() + 4);
    }

    #[test]
    fn roundtrip_json() {
        let c = sample();
        let s = serde_json::to_string_pretty(&c).unwrap();
        let back: Capsule = serde_json::from_str(&s).unwrap();
        assert_eq!(back.capsule_id, c.capsule_id);
        assert_eq!(back.source_agent, "codex");
        assert_eq!(back.target_agent.as_deref(), Some("claude-code"));
    }

    #[test]
    fn deserializes_legacy_capsule_with_agent_kind_strings() {
        // Files written by <=2.1.5 carry kebab-case AgentKind strings and a
        // consumption block without consumed_despite_target.
        let json = r#"{
            "schema_version": 2,
            "capsule_id": "cap_old",
            "project_id": "projX",
            "created_at": "2026-06-25T12:00:00Z",
            "source_agent": "claude-code",
            "target_agent": "codex",
            "session": { "session_id": null, "started_at": null, "ended_at": null, "turn_count": 0 },
            "summary": { "goal": "g", "done": [], "remaining": [], "risks": [] },
            "files": [],
            "next_prompt": null,
            "redaction": { "applied": true, "ruleset": "default-v2" },
            "consumption": { "state": "pending", "consumed_by": null, "consumed_at": null }
        }"#;
        let c: Capsule = serde_json::from_str(json).unwrap();
        assert_eq!(c.source_agent, "claude-code");
        assert_eq!(c.target_agent.as_deref(), Some("codex"));
        assert!(!c.consumption.consumed_despite_target);
    }

    #[test]
    fn deserializes_missing_or_null_target_as_open() {
        let mut v = serde_json::to_value(sample()).unwrap();
        v["target_agent"] = serde_json::Value::Null;
        let c: Capsule = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(c.target_agent, None);

        v.as_object_mut().unwrap().remove("target_agent");
        let c: Capsule = serde_json::from_value(v).unwrap();
        assert_eq!(c.target_agent, None);
    }

    #[test]
    fn serializes_open_target_as_null_keeping_the_key() {
        let mut c = sample();
        c.target_agent = None;
        let v = serde_json::to_value(&c).unwrap();
        assert!(v.as_object().unwrap().contains_key("target_agent"));
        assert!(v["target_agent"].is_null());
    }

    #[test]
    fn validate_allows_same_source_and_target() {
        let mut c = sample();
        c.target_agent = Some("codex".into());
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn validate_allows_open_target() {
        let mut c = sample();
        c.target_agent = None;
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn validate_rejects_empty_agent_ids() {
        let mut c = sample();
        c.source_agent = String::new();
        assert!(validate(&c).is_err());

        let mut c = sample();
        c.target_agent = Some(String::new());
        assert!(validate(&c).is_err());
    }

    #[test]
    fn canonical_agent_id_normalizes_known_aliases_and_passes_unknown() {
        assert_eq!(canonical_agent_id("claude").as_deref(), Some("claude-code"));
        assert_eq!(
            canonical_agent_id("Claude-Code").as_deref(),
            Some("claude-code")
        );
        assert_eq!(
            canonical_agent_id("claude_code").as_deref(),
            Some("claude-code")
        );
        assert_eq!(canonical_agent_id("codex").as_deref(), Some("codex"));
        // Unknown agents pass through normalized — forward compatibility for
        // Cursor / Gemini / Grok without touching this crate.
        assert_eq!(canonical_agent_id("Grok").as_deref(), Some("grok"));
        assert_eq!(canonical_agent_id("  gemini "), Some("gemini".into()));
        assert_eq!(canonical_agent_id(""), None);
        assert_eq!(canonical_agent_id("   "), None);
    }

    #[test]
    fn agent_kind_canonical_str_matches_serde_values() {
        assert_eq!(AgentKind::ClaudeCode.as_canonical_str(), "claude-code");
        assert_eq!(AgentKind::Codex.as_canonical_str(), "codex");
    }

    #[test]
    fn payload_hash_ignores_integrity_field() {
        let c = sample();
        let h = payload_sha256(&c);
        assert!(h.starts_with("sha256:"));
        // hashing twice is stable
        assert_eq!(h, payload_sha256(&c));
    }
}
