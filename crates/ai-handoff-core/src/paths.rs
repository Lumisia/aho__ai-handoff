use std::path::PathBuf;

/// AI Handoff home. `$AI_HANDOFF_HOME` wins; otherwise an OS-specific default.
/// Windows deliberately uses `%USERPROFILE%\.ai-handoff` (NOT %LOCALAPPDATA%)
/// to avoid the MSIX/Store AppData redirection split that gives Claude and
/// Codex different physical paths.
pub fn home() -> PathBuf {
    if let Ok(h) = std::env::var("AI_HANDOFF_HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    os_default_home()
}

#[cfg(windows)]
fn os_default_home() -> PathBuf {
    directories::BaseDirs::new()
        .as_ref()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_default()
        .join(".ai-handoff")
}

#[cfg(target_os = "macos")]
fn os_default_home() -> PathBuf {
    directories::BaseDirs::new()
        .as_ref()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_default()
        .join("Library/Application Support/ai-handoff")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn os_default_home() -> PathBuf {
    // XDG_STATE_HOME or ~/.local/state, then /ai-handoff
    if let Ok(x) = std::env::var("XDG_STATE_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x).join("ai-handoff");
        }
    }
    directories::BaseDirs::new()
        .as_ref()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_default()
        .join(".local/state/ai-handoff")
}

pub fn store_dir() -> PathBuf {
    home().join("store")
}
pub fn ipc_dir() -> PathBuf {
    home().join("ipc")
}
pub fn logs_dir() -> PathBuf {
    home().join("logs")
}
pub fn requests_dir() -> PathBuf {
    ipc_dir().join("requests")
}
pub fn responses_dir() -> PathBuf {
    ipc_dir().join("responses")
}
pub fn dead_letter_dir() -> PathBuf {
    ipc_dir().join("dead-letter")
}
pub fn project_dir(project_id: &str) -> PathBuf {
    store_dir().join("capsules").join(project_id)
}
pub fn capsule_path(project_id: &str, capsule_id: &str) -> PathBuf {
    project_dir(project_id).join(format!("{capsule_id}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Both env-mutating assertions are in a single test function to avoid
    // the race that would occur if Rust ran them in parallel threads.
    // The brief originally had two #[test] fns sharing AI_HANDOFF_HOME;
    // we merge them sequentially here — all assertions are preserved verbatim.
    #[test]
    fn home_and_layout_paths() {
        // --- home_prefers_env_then_os_default ---
        std::env::set_var("AI_HANDOFF_HOME", "/tmp/ah-test-home");
        assert_eq!(home(), PathBuf::from("/tmp/ah-test-home"));
        std::env::remove_var("AI_HANDOFF_HOME");

        let h = home();
        // OS default must end with the ai-handoff home dir name.
        if cfg!(windows) {
            assert!(
                h.ends_with(".ai-handoff"),
                "windows home = %USERPROFILE%\\.ai-handoff, got {h:?}"
            );
        } else if cfg!(target_os = "macos") {
            assert!(h.ends_with("Application Support/ai-handoff"), "got {h:?}");
        } else {
            assert!(h.ends_with("ai-handoff"), "got {h:?}");
        }

        // --- layout_paths_compose_from_home ---
        std::env::set_var("AI_HANDOFF_HOME", "/tmp/ah-layout");
        assert_eq!(store_dir(), PathBuf::from("/tmp/ah-layout/store"));
        assert_eq!(requests_dir(), PathBuf::from("/tmp/ah-layout/ipc/requests"));
        assert_eq!(
            capsule_path("projX", "capY"),
            PathBuf::from("/tmp/ah-layout/store/capsules/projX/capY.json")
        );
        std::env::remove_var("AI_HANDOFF_HOME");
    }
}
