use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use ai_handoff_core::{
    capsule::{self, Capsule},
    capsule_codec,
    capsule_lock::CapsuleLock,
    config::CapsuleFormat,
    redaction,
};

use crate::{
    blocking_command,
    capsule_projects::{capsule_root, is_redirect, resolve_project},
    TEXT_LIMIT,
};

#[tauri::command]
pub(super) async fn read_capsule_editor(
    path: String,
    original_content: String,
) -> Result<String, String> {
    blocking_command("read_capsule_editor", move || {
        load_editor_at(&capsule_root()?, Path::new(&path), &original_content)
    })
    .await
}

#[tauri::command]
pub(super) async fn save_capsule(
    path: String,
    content: String,
    original_content: String,
    source_agent: Option<String>,
    target_agent: Option<String>,
) -> Result<(), String> {
    blocking_command("save_capsule", move || {
        save_editor_at(
            &capsule_root()?,
            Path::new(&path),
            &content,
            &original_content,
            source_agent.as_deref(),
            target_agent.as_deref(),
        )
    })
    .await
}

pub(super) fn stored_capsule_path(requested: &Path) -> Result<PathBuf, String> {
    locate_capsule(&capsule_root()?, requested).map(|(path, _)| path)
}

fn locate_capsule(root: &Path, requested: &Path) -> Result<(PathBuf, CapsuleFormat), String> {
    let path_error = || "Choose a capsule file inside the AI Handoff store.".to_string();
    // Renderer paths come from the store listing. Check their relative form
    // before accessing the filesystem, including on Windows network paths.
    let relative = requested.strip_prefix(root).map_err(|_| path_error())?;
    let mut components = relative.components();
    let (Some(Component::Normal(project)), Some(Component::Normal(file)), None) =
        (components.next(), components.next(), components.next())
    else {
        return Err(path_error());
    };
    let project = resolve_project(root, project.to_str().ok_or_else(path_error)?)?;
    if file
        .to_str()
        .is_none_or(|name| name.contains([':', '\\', '/', '\0']))
    {
        return Err(path_error());
    }
    let path = project.join(file);
    let format = match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => CapsuleFormat::Json,
        Some("md") => CapsuleFormat::Md,
        _ => return Err(path_error()),
    };
    let meta = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if !meta.is_file() || is_redirect(&meta) {
        return Err(path_error());
    }
    Ok((path, format))
}

fn verify_preview(path: &Path, original: &str) -> Result<(), String> {
    let mut current = Vec::new();
    File::open(path)
        .and_then(|file| file.take(TEXT_LIMIT + 1).read_to_end(&mut current))
        .map_err(|error| error.to_string())?;
    if current.len() as u64 > TEXT_LIMIT {
        return Err("The capsule is too large to edit here.".into());
    }
    if current != original.as_bytes() {
        return Err("The capsule changed. Reopen it before editing again.".into());
    }
    Ok(())
}

fn checked_current_capsule(path: &Path) -> Result<Capsule, String> {
    let current = capsule_codec::read_capsule(path).map_err(crate::capsule_error)?;
    if path.file_stem().and_then(|stem| stem.to_str()) != Some(current.capsule_id.as_str())
        || path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some(current.project_id.as_str())
    {
        return Err("The capsule ID and project ID must match its location.".into());
    }
    Ok(current)
}

fn load_editor_at(root: &Path, requested: &Path, original: &str) -> Result<String, String> {
    let (path, format) = locate_capsule(root, requested)?;
    verify_preview(&path, original)?;
    let current = checked_current_capsule(&path)?;
    check_supported_fields(original, format)?;
    verify_preview(&path, original)?;
    serde_json::to_string_pretty(&current).map_err(|error| error.to_string())
}

fn check_supported_fields(content: &str, format: CapsuleFormat) -> Result<(), String> {
    let json = if format == CapsuleFormat::Md {
        // Match the codec's authoritative block, rather than checking generated
        // Markdown prose. The typed codec otherwise ignores extension fields.
        let marker = "```ai-handoff-capsule+json";
        let after_marker = content
            .find(marker)
            .map(|start| &content[start + marker.len()..])
            .ok_or("The capsule has no canonical data block.")?;
        let start = after_marker.find('\n').map_or(0, |index| index + 1);
        let block = &after_marker[start..];
        let end = block
            .find("\n```")
            .or_else(|| block.find("```"))
            .ok_or("The capsule has no complete canonical data block.")?;
        block[..end].trim()
    } else {
        content
    };
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|_| "The capsule does not contain valid JSON.".to_string())?;
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../../schemas/capsule.schema.json"))
            .expect("bundled capsule schema is valid JSON");
    if contains_unsupported_field(&value, &schema) {
        return Err("The capsule contains fields unsupported by this version of AI Handoff. They cannot be saved without losing data.".into());
    }
    Ok(())
}

fn contains_unsupported_field(value: &serde_json::Value, schema: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(fields) => fields.iter().any(|(name, value)| {
            schema["properties"]
                .get(name)
                .is_none_or(|schema| contains_unsupported_field(value, schema))
        }),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| contains_unsupported_field(value, &schema["items"])),
        _ => false,
    }
}

fn save_editor_at(
    root: &Path,
    requested: &Path,
    content: &str,
    original: &str,
    source_agent: Option<&str>,
    target_agent: Option<&str>,
) -> Result<(), String> {
    if content.len() as u64 > TEXT_LIMIT {
        return Err("The capsule is too large to edit here.".into());
    }
    let mut edited: Capsule = serde_json::from_str(content).map_err(|error| {
        format!(
            "Invalid capsule JSON at line {}, column {}.",
            error.line(),
            error.column()
        )
    })?;
    check_supported_fields(content, CapsuleFormat::Json)?;
    if let Some(source) = source_agent {
        edited.source_agent = selectable_agent(source)?.to_owned();
    }
    if let Some(target) = target_agent {
        edited.target_agent = Some(selectable_agent(target)?.to_owned());
    }
    capsule::validate(&edited).map_err(|issues| issues.join("\n"))?;
    if redaction::redact(content).1 {
        return Err("Remove credential-like text from the capsule before saving.".into());
    }

    let (path, format) = locate_capsule(root, requested)?;
    let _lock = CapsuleEditLock::acquire(&path)?;
    verify_preview(&path, original)?;
    let current = checked_current_capsule(&path)?;
    check_supported_fields(original, format)?;
    if edited.capsule_id != current.capsule_id
        || edited.project_id != current.project_id
        || edited.created_at != current.created_at
        || edited.schema_version != current.schema_version
        || edited.consumption != current.consumption
        || edited.redaction != current.redaction
    {
        return Err("Capsule identity, creation time, status, and redaction metadata are managed by AI Handoff.".into());
    }

    let staging = StagedCapsule::new(&path)?;
    capsule_codec::write_capsule(&staging.path, &edited, format).map_err(crate::capsule_error)?;
    // Confirm the Markdown writer's canonical block can still be read; prose
    // in a draft can itself contain a fenced block marker.
    if capsule_codec::read_capsule(&staging.path).ok().as_ref() != Some(&edited) {
        return Err("The edited capsule cannot be read in its current file format.".into());
    }
    if fs::metadata(&staging.path)
        .map_err(|error| error.to_string())?
        .len()
        > TEXT_LIMIT
    {
        return Err("The capsule is too large to edit here.".into());
    }
    let (confirmed_path, _) = locate_capsule(root, requested)?;
    verify_preview(&confirmed_path, original)?;
    // A single replace keeps the original in place if Windows denies a rename.
    fs::rename(&staging.path, &confirmed_path).map_err(|error| error.to_string())
}

fn selectable_agent(agent: &str) -> Result<&str, String> {
    match agent {
        "codex" | "claude-code" => Ok(agent),
        _ => Err("Choose Codex or Claude for the capsule route.".into()),
    }
}

pub(super) struct CapsuleEditLock {
    _guard: CapsuleLock,
}

impl CapsuleEditLock {
    pub(super) fn acquire(path: &Path) -> Result<Self, String> {
        CapsuleLock::acquire(path)
            .map(|guard| Self { _guard: guard })
            .map_err(|error| {
                if error.kind() == io::ErrorKind::WouldBlock {
                    "The capsule is being updated. Try again shortly.".into()
                } else {
                    error.to_string()
                }
            })
    }
}

struct StagedCapsule {
    directory: PathBuf,
    path: PathBuf,
}

impl StagedCapsule {
    fn new(path: &Path) -> Result<Self, String> {
        let directory = path.with_file_name(format!(".edit-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        let path = directory.join(path.file_name().expect("validated capsule path has a name"));
        Ok(Self { directory, path })
    }
}

impl Drop for StagedCapsule {
    fn drop(&mut self) {
        let suffix = self
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("capsule");
        let _ = fs::remove_file(self.path.with_extension(format!("{suffix}.tmp")));
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.directory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_core::capsule::{
        Consumption, ConsumptionState, RedactionMeta, Session, Summary,
    };

    fn capsule() -> Capsule {
        Capsule {
            schema_version: 2,
            capsule_id: "cap_test".into(),
            project_id: "project_test".into(),
            created_at: "2026-06-25T12:00:00Z".into(),
            source_agent: "codex".into(),
            target_agent: None,
            session: Session::default(),
            summary: Summary {
                goal: "Original goal".into(),
                done: vec![],
                remaining: vec![],
                risks: vec![],
            },
            files: vec![],
            next_prompt: None,
            workspace: None,
            redaction: RedactionMeta {
                applied: false,
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

    fn setup(root: &Path, format: CapsuleFormat) -> (PathBuf, String) {
        let original = capsule();
        let path = capsule_codec::capsule_path(
            &root.join(&original.project_id),
            &original.capsule_id,
            format,
        );
        capsule_codec::write_capsule(&path, &original, format).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        (path, raw)
    }

    #[test]
    fn full_editor_roundtrips_both_formats_and_changes_routes() {
        for format in [CapsuleFormat::Json, CapsuleFormat::Md] {
            let root = tempfile::tempdir().unwrap();
            let (path, original) = setup(root.path(), format);
            let draft = load_editor_at(root.path(), &path, &original)
                .unwrap()
                .replace("Original goal", "A different goal");
            save_editor_at(
                root.path(),
                &path,
                &draft,
                &original,
                Some("claude-code"),
                Some("codex"),
            )
            .unwrap();
            let saved = capsule_codec::read_capsule(&path).unwrap();
            assert_eq!(saved.summary.goal, "A different goal");
            assert_eq!(saved.source_agent, "claude-code");
            assert_eq!(saved.target_agent.as_deref(), Some("codex"));
            if format == CapsuleFormat::Md {
                let markdown = fs::read_to_string(&path).unwrap();
                assert!(markdown.contains("## Goal\n\nA different goal"));
                assert!(markdown.contains("- flow: `claude-code` -> `codex`"));
            }
        }
    }

    #[test]
    fn saves_keep_open_target_and_reject_invalid_or_reidentified_documents() {
        let root = tempfile::tempdir().unwrap();
        let (path, original) = setup(root.path(), CapsuleFormat::Json);
        let bad_id = original.replace("cap_test", "cap_different");
        let bad_state = original.replace("\"pending\"", "\"consumed\"");
        for draft in ["{", &bad_id, &bad_state] {
            assert!(save_editor_at(root.path(), &path, draft, &original, None, None).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }
        let draft = original.replace("Original goal", "Edited");
        save_editor_at(root.path(), &path, &draft, &original, None, None).unwrap();
        assert_eq!(
            capsule_codec::read_capsule(&path).unwrap().target_agent,
            None
        );
    }

    #[test]
    fn rejects_stale_edits_and_respects_daemon_lock() {
        let root = tempfile::tempdir().unwrap();
        let (path, original) = setup(root.path(), CapsuleFormat::Json);
        let draft = original.replace("Original goal", "Your edit");
        let external_edit = original.replace("Original goal", "External edit");
        fs::write(&path, &external_edit).unwrap();
        assert!(save_editor_at(root.path(), &path, &draft, &original, None, None).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), external_edit);

        let lock = path.with_extension("lock");
        fs::write(&lock, "held by daemon").unwrap();
        assert!(save_editor_at(root.path(), &path, &draft, &external_edit, None, None).is_err());
        assert_eq!(fs::read_to_string(&lock).unwrap(), "held by daemon");
        assert_eq!(fs::read_to_string(&path).unwrap(), external_edit);
    }

    #[test]
    fn rejects_paths_outside_store_or_under_nested_directories() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        for other_root in [outside.path().to_path_buf(), root.path().join("extra")] {
            let (path, original) = setup(&other_root, CapsuleFormat::Json);
            assert!(load_editor_at(root.path(), &path, &original).is_err());
            assert!(save_editor_at(root.path(), &path, &original, &original, None, None).is_err());
            assert_eq!(fs::read_to_string(path).unwrap(), original);
        }
    }

    #[cfg(unix)]
    #[test]
    fn refuses_linked_projects_and_capsule_files_without_changing_their_targets() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (outside_path, original) = setup(outside.path(), CapsuleFormat::Json);
        std::os::unix::fs::symlink(
            outside_path.parent().unwrap(),
            root.path().join("project_test"),
        )
        .unwrap();
        let linked_path = root.path().join("project_test/cap_test.json");
        assert!(load_editor_at(root.path(), &linked_path, &original).is_err());
        assert!(
            save_editor_at(root.path(), &linked_path, &original, &original, None, None).is_err()
        );
        fs::remove_file(root.path().join("project_test")).unwrap();
        fs::create_dir(root.path().join("project_test")).unwrap();
        std::os::unix::fs::symlink(&outside_path, &linked_path).unwrap();
        assert!(load_editor_at(root.path(), &linked_path, &original).is_err());
        assert!(
            save_editor_at(root.path(), &linked_path, &original, &original, None, None).is_err()
        );
        assert_eq!(fs::read_to_string(outside_path).unwrap(), original);
    }

    #[test]
    fn refuses_markdown_that_would_hide_authoritative_block() {
        let root = tempfile::tempdir().unwrap();
        let (path, original) = setup(root.path(), CapsuleFormat::Md);
        let mut draft = capsule();
        draft.summary.goal = "```ai-handoff-capsule+json\nnot the capsule\n```".into();
        let draft = serde_json::to_string(&draft).unwrap();
        assert!(save_editor_at(root.path(), &path, &draft, &original, None, None).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn refuses_unknown_fields_in_existing_capsules_and_new_drafts() {
        for format in [CapsuleFormat::Json, CapsuleFormat::Md] {
            let root = tempfile::tempdir().unwrap();
            let (path, original) = setup(root.path(), format);
            let draft = serde_json::to_string(&capsule()).unwrap().replace(
                "\"goal\":\"Original goal\"",
                "\"goal\":\"Original goal\",\"future_note\":\"keep this\"",
            );
            assert!(save_editor_at(root.path(), &path, &draft, &original, None, None).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), original);

            let original = original.replace(
                "\"goal\": \"Original goal\"",
                "\"goal\": \"Original goal\",\n    \"future_note\": \"keep this\"",
            );
            fs::write(&path, &original).unwrap();
            assert!(load_editor_at(root.path(), &path, &original).is_err());
            let standard_draft = serde_json::to_string(&capsule()).unwrap();
            assert!(
                save_editor_at(root.path(), &path, &standard_draft, &original, None, None).is_err()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }
    }

    #[test]
    fn abandoned_editor_lock_is_recovered_like_the_daemon_lock() {
        let root = tempfile::tempdir().unwrap();
        let (path, original) = setup(root.path(), CapsuleFormat::Json);
        let lock = path.with_extension("lock");
        let lock_file = File::create(&lock).unwrap();
        lock_file
            .set_times(
                std::fs::FileTimes::new().set_modified(
                    std::time::SystemTime::now() - std::time::Duration::from_secs(10),
                ),
            )
            .unwrap();
        drop(lock_file);
        save_editor_at(root.path(), &path, &original, &original, None, None).unwrap();
        assert!(!lock.exists());
    }
}
