use chrono;
use std::{
    fs::DirEntry,
    fs::OpenOptions,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

/// Returns the backup path for a file in the same directory with a timestamp suffix.
/// Base filename format:
/// `<original-filename>.ai-handoff-backup-YYYYMMDD-HHMMSS`.
///
/// This is the base path for a second. [`backup_file`] appends `-N` when that
/// path already exists, so repeated writes in one second remain distinct.
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

#[derive(Debug)]
struct BackupRecord {
    path: PathBuf,
    timestamp: chrono::NaiveDateTime,
    collision: u32,
}

/// Backs up an existing file to a timestamped backup in the same directory.
/// Same-second collisions append `-1`, `-2`, and so on without overwriting.
/// Returns `Ok(Some(backup_path))` if the file exists and was backed up.
/// Returns `Ok(None)` if the file does not exist.
pub fn backup_file(
    file: &Path,
    now: chrono::DateTime<chrono::Utc>,
) -> std::io::Result<Option<PathBuf>> {
    if !file.exists() {
        return Ok(None);
    }

    let mut source = std::fs::File::open(file)?;
    let permissions = source.metadata()?.permissions();
    let base = backup_path(file, now);
    let mut collision = 0_u32;

    loop {
        let backup = collision_path(&base, collision);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
        {
            Ok(mut destination) => {
                if let Err(error) = io::copy(&mut source, &mut destination) {
                    drop(destination);
                    let _ = std::fs::remove_file(&backup);
                    return Err(error);
                }
                drop(destination);
                if let Err(error) = std::fs::set_permissions(&backup, permissions.clone()) {
                    let _ = std::fs::remove_file(&backup);
                    return Err(error);
                }
                return Ok(Some(backup));
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                collision = collision.checked_add(1).ok_or_else(|| {
                    io::Error::new(
                        ErrorKind::AlreadyExists,
                        "backup collision suffix exhausted",
                    )
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn collision_path(base: &Path, collision: u32) -> PathBuf {
    if collision == 0 {
        return base.to_path_buf();
    }
    let name = base
        .file_name()
        .expect("backup path must have a file name")
        .to_string_lossy();
    base.with_file_name(format!("{name}-{collision}"))
}

/// Delete all but the newest `keep` backups of `file`, returning what was
/// removed.
///
/// Only regular files with a valid `YYYYMMDD-HHMMSS` stamp and optional
/// positive `-N` collision suffix are considered. Symlinks and lookalike names
/// are ignored. Ordering uses the parsed `(timestamp, collision)` pair rather
/// than mtime: the name records the install that produced the snapshot, while
/// mtime is only the moment bytes were written and says nothing about ordering
/// across a restore, a file copy, or an archive round-trip.
///
/// `protected` is never deleted, even if it sorts older than every pre-existing
/// backup. Best-effort deletion continues with newer candidates after an
/// individual failure rather than failing the install that triggered the prune.
pub fn prune_backups(
    file: &Path,
    keep: usize,
    protected: Option<&Path>,
) -> std::io::Result<Vec<PathBuf>> {
    prune_backups_with(file, keep, protected, |path| std::fs::remove_file(path))
}

fn prune_backups_with<F>(
    file: &Path,
    keep: usize,
    protected: Option<&Path>,
    mut remove: F,
) -> std::io::Result<Vec<PathBuf>>
where
    F: FnMut(&Path) -> std::io::Result<()>,
{
    let (Some(dir), Some(name)) = (file.parent(), file.file_name().and_then(|n| n.to_str())) else {
        return Ok(Vec::new());
    };
    let prefix = format!("{name}.ai-handoff-backup-");

    let mut existing: Vec<BackupRecord> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| backup_record(entry, &prefix))
        .collect();
    existing.sort_by_key(|record| (record.timestamp, record.collision));

    let protected_present =
        protected.is_some_and(|path| existing.iter().any(|record| record.path.as_path() == path));
    let minimum_survivors = keep.max(usize::from(protected_present));
    let excess = existing.len().saturating_sub(minimum_survivors);
    let mut removed = Vec::new();
    for record in existing
        .into_iter()
        .filter(|record| protected != Some(record.path.as_path()))
    {
        if removed.len() == excess {
            break;
        }
        if remove(&record.path).is_ok() {
            removed.push(record.path);
        }
    }
    Ok(removed)
}

fn backup_record(entry: DirEntry, prefix: &str) -> Option<BackupRecord> {
    let is_regular_file = entry.file_type().ok()?.is_file();
    backup_record_from_path(entry.path(), is_regular_file, prefix)
}

fn backup_record_from_path(
    path: PathBuf,
    is_regular_file: bool,
    prefix: &str,
) -> Option<BackupRecord> {
    if !is_regular_file {
        return None;
    }
    let name = path.file_name()?;
    let suffix = name.to_str()?.strip_prefix(prefix)?;
    let (timestamp, collision) = parse_backup_suffix(suffix)?;
    Some(BackupRecord {
        path,
        timestamp,
        collision,
    })
}

fn parse_backup_suffix(suffix: &str) -> Option<(chrono::NaiveDateTime, u32)> {
    const TIMESTAMP_LEN: usize = 15;
    if suffix.len() < TIMESTAMP_LEN {
        return None;
    }
    let stamp = suffix.get(..TIMESTAMP_LEN)?;
    let trailing = suffix.get(TIMESTAMP_LEN..)?;
    let timestamp = chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%d-%H%M%S").ok()?;
    let collision = if trailing.is_empty() {
        0
    } else {
        let digits = trailing.strip_prefix('-')?;
        if digits.is_empty() || digits.starts_with('0') {
            return None;
        }
        digits.parse::<u32>().ok()?
    };
    Some((timestamp, collision))
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

    #[test]
    fn same_second_backups_keep_distinct_paths_and_contents() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 26, 1, 2, 3).unwrap();

        std::fs::write(&file, "first").unwrap();
        let first = backup_file(&file, now).unwrap().unwrap();
        std::fs::write(&file, "second").unwrap();
        let second = backup_file(&file, now).unwrap().unwrap();

        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(first).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(second).unwrap(), "second");
    }

    #[test]
    fn collision_suffixes_are_sorted_numerically_for_retention() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 26, 1, 2, 3).unwrap();

        for generation in 0..12 {
            std::fs::write(&file, generation.to_string()).unwrap();
            backup_file(&file, now).unwrap().unwrap();
        }

        prune_backups(&file, 3, None).unwrap();

        let mut contents: Vec<u32> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.json.ai-handoff-backup-")
            })
            .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
            .filter_map(|text| text.parse().ok())
            .collect();
        contents.sort_unstable();
        assert_eq!(contents, vec![9, 10, 11]);
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

        let removed = prune_backups(&file, 3, None).unwrap();

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

        assert!(prune_backups(&file, KEEP_BACKUPS, None).unwrap().is_empty());
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

        prune_backups(&file, 1, None).unwrap();

        assert!(handmade.exists(), "hand-made backups are not ours to prune");
        assert!(
            sibling_backups.iter().all(|p| p.exists()),
            "another file's backups must be untouched"
        );
    }

    #[test]
    fn prune_ignores_invalid_names_and_preserves_the_current_backup() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let created = seed_backups(&file, 6);
        let invalid = dir
            .path()
            .join("settings.json.ai-handoff-backup-not-a-timestamp");
        let unicode_boundary = dir
            .path()
            .join("settings.json.ai-handoff-backup-20260726-12345é");
        let future = dir
            .path()
            .join("settings.json.ai-handoff-backup-20990101-000000");
        std::fs::write(&invalid, "manual").unwrap();
        std::fs::write(&unicode_boundary, "lookalike").unwrap();
        std::fs::write(&future, "future").unwrap();

        let protected = &created[0];
        prune_backups(&file, 3, Some(protected)).unwrap();

        assert!(protected.exists());
        assert!(invalid.exists());
        assert!(unicode_boundary.exists());
        assert!(future.exists());
        let valid_survivors =
            created.iter().filter(|path| path.exists()).count() + usize::from(future.exists());
        assert_eq!(valid_survivors, 3);
    }

    #[test]
    fn unicode_near_the_timestamp_boundary_is_rejected_without_panicking() {
        assert_eq!(parse_backup_suffix("20260726-12345é"), None);
    }

    #[test]
    fn prune_excludes_symlinks_from_the_retention_slots() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let created = seed_backups(&file, 4);
        let link = dir
            .path()
            .join("settings.json.ai-handoff-backup-20990101-000000");

        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&created[0], &link);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_file(&created[0], &link);

        if let Err(error) = &link_result {
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314)
            {
                return;
            }
        }
        link_result.unwrap();

        prune_backups(&file, 2, None).unwrap();

        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(created.iter().filter(|path| path.exists()).count(), 2);
    }

    #[test]
    fn non_regular_entries_are_rejected_before_name_parsing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("settings.json.ai-handoff-backup-20260726-010203");
        std::fs::write(&path, "target").unwrap();

        let record = backup_record_from_path(path, false, "settings.json.ai-handoff-backup-");

        assert!(record.is_none());
    }

    #[test]
    fn prune_continues_after_an_individual_delete_failure() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let created = seed_backups(&file, 5);
        let mut attempts = 0;

        let removed = prune_backups_with(&file, 2, None, |path| {
            attempts += 1;
            if attempts == 1 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "simulated locked backup",
                ))
            } else {
                std::fs::remove_file(path)
            }
        })
        .unwrap();

        assert_eq!(attempts, 4);
        assert_eq!(removed.len(), 3);
        assert_eq!(created.iter().filter(|path| path.exists()).count(), 2);
    }

    #[test]
    fn prune_on_a_file_with_no_backups_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, "{}\n").unwrap();
        assert!(prune_backups(&file, KEEP_BACKUPS, None).unwrap().is_empty());
    }
}
