use std::path::{Path, PathBuf};

use crate::capsule::Capsule;
use crate::config::CapsuleFormat;

pub fn capsule_path(project_dir: &Path, capsule_id: &str, format: CapsuleFormat) -> PathBuf {
    let ext = match format {
        CapsuleFormat::Json => "json",
        CapsuleFormat::Md => "md",
    };
    project_dir.join(format!("{capsule_id}.{ext}"))
}

pub fn read_capsule(path: &Path) -> Result<Capsule, CapsuleCodecError> {
    let bytes = std::fs::read(path).map_err(CapsuleCodecError::Io)?;
    if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
        let text = String::from_utf8(bytes).map_err(CapsuleCodecError::Utf8)?;
        let json = extract_canonical_json_block(&text).ok_or(CapsuleCodecError::MissingBlock)?;
        serde_json::from_str(json).map_err(CapsuleCodecError::Json)
    } else {
        serde_json::from_slice(&bytes).map_err(CapsuleCodecError::Json)
    }
}

pub fn write_capsule(
    path: &Path,
    capsule: &Capsule,
    format: CapsuleFormat,
) -> Result<(), CapsuleCodecError> {
    if let Some(parent) = path.parent() {
        crate::secure_fs::ensure_private_dir(parent).map_err(CapsuleCodecError::Io)?;
    }
    let bytes = match format {
        CapsuleFormat::Json => {
            serde_json::to_vec_pretty(capsule).map_err(CapsuleCodecError::Json)?
        }
        CapsuleFormat::Md => render_markdown_capsule(capsule)
            .map_err(CapsuleCodecError::Json)?
            .into_bytes(),
    };
    write_atomic(path, &bytes).map_err(CapsuleCodecError::Io)
}

#[derive(Debug)]
pub enum CapsuleCodecError {
    Io(std::io::Error),
    Utf8(std::string::FromUtf8Error),
    Json(serde_json::Error),
    MissingBlock,
}

impl std::fmt::Display for CapsuleCodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Utf8(e) => write!(f, "utf8 error: {e}"),
            Self::Json(e) => write!(f, "json error: {e}"),
            Self::MissingBlock => write!(f, "missing ai-handoff capsule json block"),
        }
    }
}

impl std::error::Error for CapsuleCodecError {}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("capsule")
    ));
    crate::secure_fs::write_private_atomic(path, &tmp, bytes)
}

fn render_markdown_capsule(capsule: &Capsule) -> Result<String, serde_json::Error> {
    let json = serde_json::to_string_pretty(capsule)?;
    let mut out = String::new();
    out.push_str("# AI Handoff Capsule\n\n");
    out.push_str(&format!("- id: `{}`\n", capsule.capsule_id));
    out.push_str(&format!("- project: `{}`\n", capsule.project_id));
    out.push_str(&format!(
        "- state: `{}`\n",
        capsule.consumption.state.as_str()
    ));
    out.push_str(&format!(
        "- flow: `{:?}` -> `{:?}`\n\n",
        capsule.source_agent, capsule.target_agent
    ));
    out.push_str("## Goal\n\n");
    out.push_str(&capsule.summary.goal);
    out.push_str("\n\n## Done\n\n");
    push_markdown_list(&mut out, &capsule.summary.done);
    out.push_str("\n## Remaining\n\n");
    push_markdown_list(&mut out, &capsule.summary.remaining);
    out.push_str("\n## Risks\n\n");
    push_markdown_list(&mut out, &capsule.summary.risks);
    if let Some(next) = &capsule.next_prompt {
        out.push_str("\n## Next Prompt\n\n");
        out.push_str(next);
        out.push('\n');
    }
    out.push_str("\n## Canonical Data\n\n```ai-handoff-capsule+json\n");
    out.push_str(&json);
    out.push_str("\n```\n");
    Ok(out)
}

fn push_markdown_list(out: &mut String, items: &[String]) {
    if items.is_empty() {
        out.push_str("- none\n");
    } else {
        for item in items {
            out.push_str("- ");
            out.push_str(item);
            out.push('\n');
        }
    }
}

fn extract_canonical_json_block(text: &str) -> Option<&str> {
    let marker = "```ai-handoff-capsule+json";
    let start = text.find(marker)? + marker.len();
    let after_marker = &text[start..];
    let content_start = after_marker.find('\n').map(|idx| idx + 1).unwrap_or(0);
    let content = &after_marker[content_start..];
    let end = content.find("\n```").or_else(|| content.find("```"))?;
    Some(content[..end].trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capsule::{
        AgentKind, Consumption, ConsumptionState, RedactionMeta, Session, Summary,
    };

    fn sample() -> Capsule {
        Capsule {
            schema_version: 2,
            capsule_id: "cap_test".into(),
            project_id: "projX".into(),
            created_at: "2026-06-25T12:00:00Z".into(),
            source_agent: AgentKind::Codex,
            target_agent: AgentKind::ClaudeCode,
            session: Session::default(),
            summary: Summary {
                goal: "ship md".into(),
                done: vec!["done one".into()],
                remaining: vec!["remaining one".into()],
                risks: vec!["risk one".into()],
            },
            files: vec![],
            next_prompt: Some("continue".into()),
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
    fn md_capsule_round_trips_from_canonical_json_block() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap_test.md");
        let capsule = sample();

        write_capsule(&path, &capsule, CapsuleFormat::Md).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();

        assert!(text.contains("# AI Handoff Capsule"));
        assert!(text.contains("```ai-handoff-capsule+json"));
        assert!(text.contains("\"capsule_id\": \"cap_test\""));
        assert_eq!(read_capsule(&path).unwrap(), capsule);
    }

    #[test]
    fn json_capsule_round_trips_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap_test.json");
        let capsule = sample();

        write_capsule(&path, &capsule, CapsuleFormat::Json).unwrap();

        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .trim_start()
            .starts_with('{'));
        assert_eq!(read_capsule(&path).unwrap(), capsule);
    }

    #[test]
    fn capsule_path_uses_requested_extension() {
        assert_eq!(
            capsule_path(Path::new("/store/proj"), "cap_1", CapsuleFormat::Json),
            PathBuf::from("/store/proj/cap_1.json")
        );
        assert_eq!(
            capsule_path(Path::new("/store/proj"), "cap_1", CapsuleFormat::Md),
            PathBuf::from("/store/proj/cap_1.md")
        );
    }
}
