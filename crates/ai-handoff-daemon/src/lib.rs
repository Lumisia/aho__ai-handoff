pub mod dedupe;
pub mod router;
pub mod store;
pub mod trigger_mark;

use std::time::Duration;

pub fn ensure_runtime_dirs() -> std::io::Result<()> {
    ai_handoff_core::secure_fs::ensure_private_dir(&ai_handoff_core::paths::home())?;
    ai_handoff_core::secure_fs::ensure_private_dir(&ai_handoff_core::paths::ipc_dir())?;
    // The IPC subdirs must INHERIT the (private) IPC root ACL instead of
    // being hardened: on Windows the Codex sandbox's ACE lives on the root,
    // and `/inheritance:r` on the subdirs locked sandboxed hooks out of IPC
    // (every hook degraded to daemon_unavailable). These calls also repair
    // installs broken by older versions.
    ai_handoff_core::secure_fs::ensure_inherited_subdir(&ai_handoff_core::paths::requests_dir())?;
    ai_handoff_core::secure_fs::ensure_inherited_subdir(&ai_handoff_core::paths::responses_dir())?;
    ai_handoff_core::secure_fs::ensure_inherited_subdir(&ai_handoff_core::paths::dead_letter_dir())?;
    ai_handoff_core::secure_fs::ensure_private_dir(&ai_handoff_core::paths::store_dir())?;
    ai_handoff_core::secure_fs::ensure_private_dir(&ai_handoff_core::paths::logs_dir())?;
    ai_handoff_core::secure_fs::touch_private_file(
        &ai_handoff_core::paths::logs_dir().join("daemon.log"),
    )?;
    Ok(())
}

pub fn run(stay_alive: bool) -> i32 {
    let _ = ensure_runtime_dirs();
    // A daemon that cannot write the capsule store cannot do its job (capsule
    // saves, consume-state flips), and once it holds the singleton lock it
    // BLOCKS a healthy daemon from starting. A sandboxed spawn (e.g. a Codex
    // hook autostart) hits exactly this, so fail fast BEFORE taking the lock —
    // a later unsandboxed spawn can then own this home.
    if let Err(error) = store_write_preflight() {
        log_daemon(&format!(
            "fatal: capsule store is not writable ({}): {error}",
            ai_handoff_core::paths::store_dir().display()
        ));
        eprintln!(
            "[ai-handoff-daemon] capsule store is not writable: {error}; \
             start the daemon outside the sandbox or set AI_HANDOFF_HOME to a writable path"
        );
        return 3;
    }
    // Single instance per AI_HANDOFF_HOME: hook clients auto-spawn a daemon
    // whenever one looks unavailable, so concurrent spawns are normal. Extras
    // must exit instead of each polling the same request dir forever.
    let Some(_lock) = acquire_singleton_lock() else {
        std::process::exit(0);
    };
    log_daemon("daemon started");
    let router = router::Router::new();
    if stay_alive {
        ai_handoff_ipc::server::serve_forever(&router, Duration::from_millis(25));
    }
    let cfg = ai_handoff_core::config::load();
    ai_handoff_ipc::server::serve_until_idle(
        &router,
        Duration::from_millis(25),
        Duration::from_secs(cfg.daemon.idle_timeout_seconds()),
    );
    log_daemon("daemon idle exit");
    0
}

/// Prove the capsule store accepts writes by creating and removing a probe
/// file. Runs at startup (fail fast) and on demand for ping health reporting.
pub fn store_write_preflight() -> std::io::Result<()> {
    let dir = ai_handoff_core::paths::store_dir();
    let probe = dir.join(format!(".write-probe-{}", std::process::id()));
    std::fs::write(&probe, b"probe")?;
    std::fs::remove_file(&probe)
}

/// Append a timestamped line to `<home>/logs/daemon.log`. Best effort: a
/// daemon must never die because its log is unwritable, so failures are
/// swallowed. Rotates to `daemon.log.1` past 512 KiB.
pub fn log_daemon(message: &str) {
    use std::io::Write;
    let path = ai_handoff_core::paths::logs_dir().join("daemon.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 512 * 1024 {
            let _ = std::fs::rename(&path, path.with_extension("log.1"));
        }
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let _ = writeln!(file, "{now} {message}");
}

/// Take an exclusive advisory lock on `<home>/ipc/daemon.lock`. The lock is
/// held for the process lifetime and released by the OS on any exit (including
/// crashes), so no stale-lock cleanup is needed. `None` means another live
/// daemon already holds it.
pub fn acquire_singleton_lock() -> Option<std::fs::File> {
    let path = ai_handoff_core::paths::ipc_dir().join("daemon.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    match file.try_lock() {
        Ok(()) => Some(file),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_and_daemon_log_work_on_writable_home() {
        let _guard = test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        ensure_runtime_dirs().unwrap();

        store_write_preflight().expect("writable store passes preflight");
        // The probe must not leave litter behind.
        assert!(std::fs::read_dir(ai_handoff_core::paths::store_dir())
            .unwrap()
            .flatten()
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".write-probe")));

        log_daemon("test line");
        let logged =
            std::fs::read_to_string(ai_handoff_core::paths::logs_dir().join("daemon.log")).unwrap();
        assert!(logged.contains("test line"), "{logged}");

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[cfg(unix)]
    #[test]
    fn preflight_fails_on_readonly_store() {
        use std::os::unix::fs::PermissionsExt;
        let _guard = test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        ensure_runtime_dirs().unwrap();

        let store = ai_handoff_core::paths::store_dir();
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o500)).unwrap();
        assert!(store_write_preflight().is_err());
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn singleton_lock_blocks_second_holder_until_released() {
        let _guard = test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        ensure_runtime_dirs().unwrap();

        let first = acquire_singleton_lock().expect("first lock");
        assert!(
            acquire_singleton_lock().is_none(),
            "second daemon must not acquire the lock while the first is alive"
        );

        drop(first);
        assert!(
            acquire_singleton_lock().is_some(),
            "lock must be reacquirable after the holder exits"
        );
        std::env::remove_var("AI_HANDOFF_HOME");
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}
