//! Preserve a pre-plugin Codex bundle without leaving it in skill discovery.
//! Claude's `.claude/skills/ai-handoff` is its current bundle and is never moved.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, serde::Serialize)]
pub struct LegacySkills {
    pub status: &'static str,
    pub path: PathBuf,
}

pub(super) fn plain_metadata(path: &Path) -> io::Result<Option<fs::Metadata>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if linked {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "legacy skill migration does not follow links or junctions",
        ));
    }
    Ok(Some(metadata))
}

fn plain_file(root: &Path, relative: &str) -> io::Result<Option<String>> {
    let mut path = root.to_path_buf();
    let components: Vec<_> = Path::new(relative).components().collect();
    for (index, component) in components.iter().enumerate() {
        path.push(component);
        let Some(metadata) = plain_metadata(&path)? else {
            return Ok(None);
        };
        if index + 1 == components.len() {
            return if metadata.is_file() {
                fs::read_to_string(path).map(Some)
            } else {
                Ok(None)
            };
        }
        if !metadata.is_dir() {
            return Ok(None);
        }
    }
    Ok(None)
}

fn inspect_status(skills_root: &Path) -> io::Result<&'static str> {
    // This guard prevents accidental use on Claude's canonical skill bundle.
    if skills_root.file_name() != Some(std::ffi::OsStr::new("skills"))
        || skills_root.parent().and_then(Path::file_name) != Some(std::ffi::OsStr::new(".agents"))
    {
        return Ok("unsafe");
    }
    let bundle = skills_root.join("ai-handoff");
    for path in [skills_root.parent().unwrap(), skills_root, &bundle] {
        let Some(metadata) = plain_metadata(path)? else {
            return Ok("absent");
        };
        if !metadata.is_dir() {
            return Ok("unrecognized");
        }
    }
    let mut recognized = false;
    for manifest in [".claude-plugin/plugin.json", ".codex-plugin/plugin.json"] {
        if let Some(text) = plain_file(&bundle, manifest)? {
            recognized |= serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .is_some_and(|value| value["name"] == "ai-handoff");
        }
    }
    if !recognized {
        return Ok("unrecognized");
    }
    for skill in [
        "handoff",
        "handoff-config",
        "handoff-doctor",
        "handoff-checkpoint",
    ] {
        if plain_file(&bundle, &format!("skills/{skill}/SKILL.md"))?.is_some() {
            return Ok("recognized");
        }
    }
    Ok("unrecognized")
}

/// Read-only, best-effort diagnosis. Unreadable/linked bundles are preserved.
pub fn inspect(skills_root: &Path) -> LegacySkills {
    LegacySkills {
        status: inspect_status(skills_root).unwrap_or("unsafe"),
        path: skills_root.join("ai-handoff"),
    }
}

fn archive_root_exists(root: &Path) -> io::Result<bool> {
    match plain_metadata(root)? {
        Some(metadata) if metadata.is_dir() => Ok(true),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "legacy skill archive root is not a directory",
        )),
        None => Ok(false),
    }
}

/// Reject an already-blocked migration before the installer writes its config.
pub(super) fn validate_archive_destination(skills_root: &Path) -> io::Result<()> {
    if inspect(skills_root).status == "recognized" {
        let root = skills_root.parent().unwrap().join("legacy-skill-backups");
        archive_root_exists(&root)?;
    }
    Ok(())
}

/// Move only the recognized Codex residue, keeping all modified/extra files.
/// The caller must install the replacement plugin successfully first.
pub fn archive(skills_root: &Path) -> io::Result<Option<PathBuf>> {
    let legacy = inspect(skills_root);
    if legacy.status != "recognized" {
        return Ok(None);
    }
    let root = skills_root.parent().unwrap().join("legacy-skill-backups");
    if !archive_root_exists(&root)? {
        fs::create_dir(&root)?;
    }
    // Reserve a unique container: rename never replaces an existing archive.
    let container = root.join(format!("ai-handoff-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&container)?;
    let destination = container.join("ai-handoff");
    // Recheck the controlled source/destination directories before the move.
    if inspect(skills_root).status != "recognized" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "legacy skills changed during migration",
        ));
    }
    plain_metadata(&root)?;
    plain_metadata(&container)?;
    fs::rename(&legacy.path, &destination)?;
    Ok(Some(destination))
}
