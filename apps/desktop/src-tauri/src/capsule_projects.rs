use std::{
    collections::hash_map::DefaultHasher,
    ffi::OsString,
    fs,
    hash::{Hash, Hasher},
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use ai_handoff_core::{capsule_codec, paths};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct CapsuleProjectInfo {
    path: String,
    capsule_count: usize,
    revision: String,
}

#[tauri::command]
pub(super) async fn get_capsule_project(project_id: String) -> Result<CapsuleProjectInfo, String> {
    super::blocking_command("get_capsule_project", move || {
        get_project_at(&capsule_root()?, &project_id)
    })
    .await
}

#[tauri::command]
pub(super) async fn delete_capsule_project(
    project_id: String,
    expected_revision: String,
) -> Result<(), String> {
    super::blocking_command("delete_capsule_project", move || {
        delete_project_at(&capsule_root()?, &project_id, &expected_revision)
    })
    .await
}

#[tauri::command]
pub(super) async fn open_capsule_project(
    project_id: String,
) -> Result<super::MenuCommandResult, String> {
    super::blocking_command("open_capsule_project", move || {
        let root = capsule_root()?;
        resolve_project(&root, &project_id)?;
        let path = display_path(&root, &project_id)?;
        super::open_target(&path)?;
        Ok(super::MenuCommandResult {
            message: format!("opened folder: {path}"),
        })
    })
    .await
}

pub(super) fn capsule_root() -> Result<PathBuf, String> {
    let store = paths::store_dir();
    real_directory(&store)?;
    Ok(store.join("capsules"))
}

pub(super) fn is_redirect(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn real_directory(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || is_redirect(&metadata) {
        return Err("capsule project path is not an ordinary directory".into());
    }
    Ok(metadata)
}

pub(super) fn resolve_project(root: &Path, project_id: &str) -> Result<PathBuf, String> {
    if project_id.is_empty()
        || !project_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err("invalid capsule project ID".into());
    }
    real_directory(root)?;
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let candidate = root.join(project_id);
    real_directory(&candidate)?;
    let resolved = candidate
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if resolved.parent() != Some(root.as_path()) {
        return Err("capsule project is outside the capsule store".into());
    }
    Ok(resolved)
}

fn display_path(root: &Path, project_id: &str) -> Result<String, String> {
    // The file manager and clipboard expect a normal Windows path, without
    // the verbatim prefix returned by canonicalize().
    std::path::absolute(root.join(project_id))
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

struct ProjectSnapshot {
    files: Vec<OsString>,
    capsule_count: usize,
    revision: String,
}

fn snapshot(path: &Path, project_id: &str) -> Result<ProjectSnapshot, String> {
    let project = real_directory(path)?;
    let mut files = fs::read_dir(path)
        .and_then(|entries| {
            entries
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect::<io::Result<Vec<_>>>()
        })
        .map_err(|error| error.to_string())?;
    files.sort_unstable();
    let mut digest = DefaultHasher::new();
    project_id.hash(&mut digest);
    project.created().ok().hash(&mut digest);
    let mut capsule_count = 0;
    for name in &files {
        let file = path.join(name);
        let metadata = fs::symlink_metadata(&file).map_err(|error| error.to_string())?;
        if !metadata.is_file() || is_redirect(&metadata) {
            return Err("capsule project contains a link or another non-file entry".into());
        }
        // Daemon/Tauri edit locks and atomic writes live beside their objects.
        // Allow cleanup of abandoned artifacts, but do not stage recent writes.
        if matches!(
            file.extension().and_then(|ext| ext.to_str()),
            Some("lock" | "tmp")
        ) && metadata
            .modified()
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_none_or(|age| age < Duration::from_secs(30))
        {
            return Err("capsule project is being updated; try again".into());
        }
        name.hash(&mut digest);
        metadata.created().ok().hash(&mut digest);
        metadata.modified().ok().hash(&mut digest);
        fs::read(&file)
            .map_err(|error| error.to_string())?
            .hash(&mut digest);
        if matches!(
            file.extension().and_then(|ext| ext.to_str()),
            Some("json" | "md")
        ) && capsule_codec::read_capsule(&file).is_ok()
        {
            capsule_count += 1;
        }
    }
    Ok(ProjectSnapshot {
        files,
        capsule_count,
        revision: format!("{:016x}", digest.finish()),
    })
}

fn get_project_at(root: &Path, project_id: &str) -> Result<CapsuleProjectInfo, String> {
    let directory = resolve_project(root, project_id)?;
    let snapshot = snapshot(&directory, project_id)?;
    Ok(CapsuleProjectInfo {
        path: display_path(root, project_id)?,
        capsule_count: snapshot.capsule_count,
        revision: snapshot.revision,
    })
}

fn check_revision(snapshot: ProjectSnapshot, expected: &str) -> Result<ProjectSnapshot, String> {
    if snapshot.revision != expected {
        return Err(
            "capsule project changed since confirmation; reopen the deletion dialog".into(),
        );
    }
    Ok(snapshot)
}

fn delete_project_at(root: &Path, project_id: &str, expected_revision: &str) -> Result<(), String> {
    let directory = resolve_project(root, project_id)?;
    check_revision(snapshot(&directory, project_id)?, expected_revision)?;
    let root = directory
        .parent()
        .ok_or("capsule project has no parent folder")?;
    let staging = root.join(format!(".delete-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging).map_err(|error| error.to_string())?;
    let moved = staging.join(project_id);
    // A later write may recreate the original project folder. It must never
    // be part of the confirmed deletion. Recheck the moved folder so a write
    // between the first snapshot and rename does not expand this deletion.
    if let Err(error) = fs::rename(&directory, &moved) {
        let _ = fs::remove_dir(&staging);
        return Err(error.to_string());
    }
    let confirmed = match snapshot(&moved, project_id)
        .and_then(|current| check_revision(current, expected_revision))
    {
        Ok(confirmed) => confirmed,
        Err(error) => {
            if fs::symlink_metadata(&directory)
                .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
                && fs::rename(&moved, &directory).is_ok()
            {
                let _ = fs::remove_dir(&staging);
                return Err(error);
            }
            return Err(format!(
                "{error}. Project files were kept in {}",
                moved.display()
            ));
        }
    };
    let clean_up = || -> io::Result<()> {
        for file in confirmed.files {
            fs::remove_file(moved.join(file))?;
        }
        fs::remove_dir(&moved)?;
        fs::remove_dir(&staging)
    };
    clean_up().map_err(|error| {
        format!(
            "could not finish deleting capsule project: {error}. Check {}",
            staging.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_core::{
        capsule::{Capsule, Consumption, ConsumptionState, RedactionMeta, Session, Summary},
        config::CapsuleFormat,
    };

    fn write_capsule(path: &Path, id: &str, source: &str, format: CapsuleFormat) {
        capsule_codec::write_capsule(
            path,
            &Capsule {
                schema_version: 2,
                capsule_id: id.into(),
                project_id: "mixed-project".into(),
                created_at: "2026-11-16T09:00:00Z".into(),
                source_agent: source.into(),
                target_agent: None,
                session: Session::default(),
                summary: Summary {
                    goal: "Test capsule".into(),
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
            },
            format,
        )
        .unwrap();
    }

    #[test]
    fn project_deletion_counts_both_agents_and_formats_and_removes_only_its_folder() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("mixed-project");
        write_capsule(
            &project.join("cap_one.json"),
            "cap_one",
            "codex",
            CapsuleFormat::Json,
        );
        write_capsule(
            &project.join("cap_two.md"),
            "cap_two",
            "claude-code",
            CapsuleFormat::Md,
        );
        fs::write(project.join("project.label"), "mixed-project").unwrap();
        let sibling = root.path().join("other-project");
        fs::create_dir(&sibling).unwrap();
        fs::write(sibling.join("keep"), "sibling data").unwrap();

        let info = get_project_at(root.path(), "mixed-project").unwrap();
        assert_eq!(info.capsule_count, 2);
        assert_eq!(
            Path::new(&info.path).canonicalize().unwrap(),
            project.canonicalize().unwrap()
        );
        delete_project_at(root.path(), "mixed-project", &info.revision).unwrap();
        assert!(!project.exists());
        assert_eq!(
            fs::read_to_string(sibling.join("keep")).unwrap(),
            "sibling data"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn project_deletion_rejects_same_count_replacement_and_new_capsules() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("mixed-project");
        let first = project.join("cap_one.json");
        write_capsule(&first, "cap_one", "codex", CapsuleFormat::Json);
        let stale = get_project_at(root.path(), "mixed-project").unwrap();
        write_capsule(&first, "cap_two", "codex", CapsuleFormat::Json);
        let current = get_project_at(root.path(), "mixed-project").unwrap();
        assert_eq!(stale.capsule_count, current.capsule_count);
        assert!(delete_project_at(root.path(), "mixed-project", &stale.revision).is_err());
        assert_eq!(
            capsule_codec::read_capsule(&first).unwrap().capsule_id,
            "cap_two"
        );

        let second = project.join("cap_three.json");
        write_capsule(&second, "cap_three", "claude-code", CapsuleFormat::Json);
        assert!(delete_project_at(root.path(), "mixed-project", &current.revision).is_err());
        assert!(first.is_file() && second.is_file());
    }

    #[test]
    fn project_deletion_rejects_traversal_and_unsafe_members() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path().join("capsules");
        let project = root.join("mixed-project");
        let nested = project.join("unexpected");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("marker"), "not a capsule").unwrap();
        let outside = store.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("marker"), "not a capsule").unwrap();

        for id in [
            "",
            ".",
            "..",
            "../outside",
            "..\\outside",
            "C:outside",
            "mixed-project.",
        ] {
            assert!(get_project_at(&root, id).is_err(), "{id:?}");
            assert!(delete_project_at(&root, id, "").is_err(), "{id:?}");
        }
        assert!(get_project_at(&root, "mixed-project").is_err());
        assert!(nested.join("marker").is_file());
        assert!(outside.join("marker").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn project_deletion_rejects_a_link_to_an_outside_folder() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("marker"), "keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("linked-project")).unwrap();
        assert!(get_project_at(root.path(), "linked-project").is_err());
        assert!(delete_project_at(root.path(), "linked-project", "").is_err());
        assert!(outside.path().join("marker").is_file());
    }

    #[test]
    fn project_deletion_leaves_capsules_in_place_when_a_writer_is_active() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("mixed-project");
        let capsule = project.join("cap_one.json");
        write_capsule(&capsule, "cap_one", "codex", CapsuleFormat::Json);
        let confirmed = get_project_at(root.path(), "mixed-project").unwrap();
        fs::write(capsule.with_extension("lock"), "").unwrap();
        assert!(delete_project_at(root.path(), "mixed-project", &confirmed.revision).is_err());
        assert!(capsule.is_file());
    }
}
