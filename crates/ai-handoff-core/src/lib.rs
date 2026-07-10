// modules added per task
pub mod account;
pub mod account_api;
pub mod capsule;
pub mod capsule_codec;
pub mod checkpoint_input;
pub mod config;
pub mod dashboard;
pub mod fingerprint;
pub mod git_info;
pub mod hook_event;
pub mod install;
pub mod keychain;
pub mod paths;
pub mod process;
pub mod redaction;
pub mod secure_fs;
pub mod sensor;
pub mod statusline;
pub mod trigger;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}
