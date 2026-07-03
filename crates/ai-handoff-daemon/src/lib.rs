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
    // Single instance per AI_HANDOFF_HOME: hook clients auto-spawn a daemon
    // whenever one looks unavailable, so concurrent spawns are normal. Extras
    // must exit instead of each polling the same request dir forever.
    let Some(_lock) = acquire_singleton_lock() else {
        std::process::exit(0);
    };
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
    0
}

/// Take an exclusive advisory lock on `<home>/ipc/daemon.lock`. The lock is
/// held for the process lifetime and released by the OS on any exit (including
/// crashes), so no stale-lock cleanup is needed. `None` means another live
/// daemon already holds it.
pub fn acquire_singleton_lock() -> Option<std::fs::File> {
    let path = ai_handoff_core::paths::ipc_dir().join("daemon.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
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
