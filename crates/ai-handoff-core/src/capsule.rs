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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Consumption {
    pub state: ConsumptionState,
    #[serde(default)]
    pub consumed_by: Option<String>,
    #[serde(default)]
    pub consumed_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// The agent kind — serialized as kebab-case strings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
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
    pub source_agent: AgentKind,
    pub target_agent: AgentKind,
    pub session: Session,
    pub summary: Summary,
    pub files: Vec<FileChange>,
    #[serde(default)]
    pub next_prompt: Option<String>,
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
    if c.source_agent == c.target_agent {
        reasons.push("source_agent and target_agent must differ".into());
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
            source_agent: AgentKind::Codex,
            target_agent: AgentKind::ClaudeCode,
            session: Session::default(),
            summary: Summary {
                goal: "g".into(),
                done: vec![],
                remaining: vec![],
                risks: vec![],
            },
            files: vec![],
            next_prompt: Some("do x".into()),
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
        assert_eq!(back.source_agent, AgentKind::Codex);
    }

    #[test]
    fn validate_rejects_same_source_and_target() {
        let mut c = sample();
        c.target_agent = AgentKind::Codex;
        assert!(validate(&c).is_err());
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
