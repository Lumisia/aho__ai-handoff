use chrono;
use std::path::{Path, PathBuf};

/// Returns the backup path for a file in the same directory with a timestamp suffix.
/// Filename format: `<original-filename>.ai-handoff-backup-YYYYMMDD-HHMMSS`
///
/// Resolution is one second, so two writes to the same file inside the same
/// second reuse one backup name and the later copy wins. Acceptable: a single
/// install writes each file at most once, and the surviving copy is still a
/// pre-write snapshot.
pub fn backup_path(file: &Path, now: chrono::DateTime<chrono::Utc>) -> PathBuf {
    let timestamp = now.format("%Y%m%d-%H%M%S").to_string();
    let file_name = file.file_name().expect("file must have a name");
    let backup_name = format!(
        "{}.ai-handoff-backup-{}",
        file_name.to_string_lossy(),
        timestamp
    );
    file.parent()
        .expect("file must have a parent directory")
        .join(backup_name)
}

/// How many timestamped backups to keep per file.
///
/// Backups are a manual escape hatch — nothing reads them programmatically, and
/// uninstall restores surgically rather than from a snapshot. They earn their
/// keep when the merge logic itself is wrong (atomic writes guarantee a
/// *complete* file, not a *correct* one), so a short history is worth holding;
/// an unbounded one is not.
pub const KEEP_BACKUPS: usize = 5;

/// Backs up an existing file to a timestamped backup in the same directory.
/// Returns `Ok(Some(backup_path))` if the file exists and was backed up.
/// Returns `Ok(None)` if the file does not exist.
pub fn backup_file(
    file: &Path,
    now: chrono::DateTime<chrono::Utc>,
) -> std::io::Result<Option<PathBuf>> {
    if !file.exists() {
        return Ok(None);
    }

    let backup = backup_path(file, now);
    std::fs::copy(file, &backup)?;
    Ok(Some(backup))
}

/// Delete all but the newest `keep` backups of `file`, returning what was
/// removed.
///
/// Ordering comes from the `YYYYMMDD-HHMMSS` stamp embedded in the filename,
/// never from mtime: [`backup_file`] copies with metadata, so every backup
/// carries the *original* file's timestamp and mtime order says nothing about
/// when the backup was taken.
///
/// Best-effort — a backup we cannot remove is skipped rather than failing the
/// install that triggered the prune.
pub fn prune_backups(file: &Path, keep: usize) -> std::io::Result<Vec<PathBuf>> {
    let (Some(dir), Some(name)) = (file.parent(), file.file_name().and_then(|n| n.to_str())) else {
        return Ok(Vec::new());
    };
    let prefix = format!("{name}.ai-handoff-backup-");

    let mut existing: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    existing.sort();

    let excess = existing.len().saturating_sub(keep);
    let mut removed = Vec::new();
    for path in existing.into_iter().take(excess) {
        if std::fs::remove_file(&path).is_ok() {
            removed.push(path);
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn backs_up_existing_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("config.toml");
        std::fs::write(&f, "model = \"x\"\n").unwrap();
        let now = chrono::Utc.with_ymd_and_hms(2026, 6, 25, 1, 2, 3).unwrap();
        let b = backup_file(&f, now).unwrap().unwrap();
        assert!(b
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("ai-handoff-backup-20260625-010203"));
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "model = \"x\"\n");
        // absent file -> None
        assert!(backup_file(&dir.path().join("nope.json"), now)
            .unwrap()
            .is_none());
    }

    /// Create `count` backups of `file` one second apart, oldest first.
    fn seed_backups(file: &Path, count: u32) -> Vec<PathBuf> {
        (0..count)
            .map(|i| {
                let now = chrono::Utc.with_ymd_and_hms(2026, 6, 25, 1, 2, i).unwrap();
                std::fs::write(file, format!("gen {i}\n")).unwrap();
                backup_file(file, now).unwrap().unwrap()
            })
            .collect()
    }

    #[test]
    fn prune_keeps_the_newest_backups_by_embedded_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let created = seed_backups(&file, 8);

        let removed = prune_backups(&file, 3).unwrap();

        assert_eq!(removed.len(), 5, "8 backups, keep 3");
        for stale in &created[..5] {
            assert!(!stale.exists(), "{stale:?} should be pruned");
        }
        for kept in &created[5..] {
            assert!(kept.exists(), "{kept:?} should survive");
        }
    }

    #[test]
    fn prune_is_a_no_op_below_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let created = seed_backups(&file, 2);

        assert!(prune_backups(&file, KEEP_BACKUPS).unwrap().is_empty());
        assert!(created.iter().all(|p| p.exists()));
    }

    #[test]
    fn prune_ignores_unrelated_files_and_other_backup_families() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        seed_backups(&file, 4);

        let sibling = dir.path().join("config.toml");
        std::fs::write(&sibling, "model = \"x\"\n").unwrap();
        let sibling_backups = seed_backups(&sibling, 4);
        let handmade = dir.path().join("settings.json.bak-manual");
        std::fs::write(&handmade, "manual\n").unwrap();

        prune_backups(&file, 1).unwrap();

        assert!(handmade.exists(), "hand-made backups are not ours to prune");
        assert!(
            sibling_backups.iter().all(|p| p.exists()),
            "another file's backups must be untouched"
        );
    }

    #[test]
    fn prune_on_a_file_with_no_backups_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, "{}\n").unwrap();
        assert!(prune_backups(&file, KEEP_BACKUPS).unwrap().is_empty());
    }
}
