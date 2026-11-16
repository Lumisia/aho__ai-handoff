//! Advisory locking for capsule read-modify-write operations. The daemon uses
//! the same `.lock` path and expiry, so CLI/TUI/GUI writers serialize together.

use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct CapsuleLock(PathBuf);

impl CapsuleLock {
    /// Acquire the lock shared by JSON and Markdown representations of an ID.
    /// A crashed writer's ordinary lock file is recovered after five seconds.
    pub fn acquire(capsule_path: &Path) -> io::Result<Self> {
        let lock = capsule_path.with_extension("lock");
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            match OpenOptions::new().create_new(true).write(true).open(&lock) {
                Ok(_) => return Ok(Self(lock)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let stale = fs::symlink_metadata(&lock)
                        .ok()
                        .filter(|metadata| metadata.is_file() && !is_redirect(metadata))
                        .and_then(|metadata| metadata.modified().ok())
                        .and_then(|modified| modified.elapsed().ok())
                        .is_some_and(|age| age > Duration::from_secs(5));
                    if stale && fs::remove_file(&lock).is_ok() {
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "capsule is locked by another process",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn is_redirect(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

impl Drop for CapsuleLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_both_formats_until_the_guard_is_dropped() {
        let root = tempfile::tempdir().unwrap();
        let json = root.path().join("cap_test.json");
        let markdown = json.with_extension("md");
        let guard = CapsuleLock::acquire(&json).unwrap();
        assert!(
            matches!(CapsuleLock::acquire(&markdown), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        );
        assert!(json.with_extension("lock").is_file());
        drop(guard);
        assert!(!json.with_extension("lock").exists());
        let _guard = CapsuleLock::acquire(&markdown).unwrap();
    }

    #[test]
    fn recovers_an_abandoned_lock_using_the_daemons_expiry() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cap_test.json");
        let lock = path.with_extension("lock");
        let file = fs::File::create(&lock).unwrap();
        file.set_times(
            fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() - Duration::from_secs(10)),
        )
        .unwrap();
        drop(file);
        let guard = CapsuleLock::acquire(&path).unwrap();
        assert!(lock.is_file());
        drop(guard);
        assert!(!lock.exists());
    }
}
