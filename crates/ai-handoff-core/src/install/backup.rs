use chrono;
use std::path::{Path, PathBuf};

/// Returns the backup path for a file in the same directory with a timestamp suffix.
/// Filename format: `<original-filename>.ai-handoff-backup-YYYYMMDD-HHMMSS`
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
}
