use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct FileMod {
    pub path: String,
    pub backup: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct CodexState {
    pub hooks_file: Option<FileMod>,
    pub config_file: Option<FileMod>,
    pub managed_hook_events: Vec<String>,
    pub writable_root_added: Option<String>,
    pub created_sandbox_table: bool,
    pub env_key_added: Option<String>,
    pub created_env_table: bool,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct ClaudeState {
    pub settings_file: Option<FileMod>,
    pub managed_hook_events: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutostartKind {
    ScheduledTask,
    HkcuRun,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AutostartState {
    pub kind: AutostartKind,
    pub name: String,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct LauncherState {
    pub path: Option<String>,
    pub path_dir_added: Option<String>,
}

impl AutostartState {
    pub fn new(kind: AutostartKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstallState {
    pub version: u32,
    pub installed_at: String,
    pub codex: CodexState,
    pub claude: ClaudeState,
    #[serde(default)]
    pub autostart: Option<AutostartState>,
    #[serde(default)]
    pub scheduled_task: Option<String>,
    #[serde(default)]
    pub launcher: Option<LauncherState>,
}

impl Default for InstallState {
    fn default() -> Self {
        Self {
            version: 1,
            installed_at: String::new(),
            codex: CodexState::default(),
            claude: ClaudeState::default(),
            autostart: None,
            scheduled_task: None,
            launcher: None,
        }
    }
}

pub fn state_path(ai_home: &Path) -> PathBuf {
    ai_home.join("install-state.json")
}

pub fn load(ai_home: &Path) -> InstallState {
    let p = state_path(ai_home);
    let bytes = match std::fs::read(&p) {
        Ok(b) => b,
        Err(_) => return InstallState::default(),
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save(ai_home: &Path, st: &InstallState) -> std::io::Result<()> {
    std::fs::create_dir_all(ai_home)?;
    let p = state_path(ai_home);
    let tmp = p.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(st)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, &json)?;
    match std::fs::rename(&tmp, &p) {
        Ok(()) => Ok(()),
        Err(error) if p.exists() => {
            std::fs::remove_file(&p)?;
            std::fs::rename(&tmp, &p).map_err(|second_error| {
                let _ = std::fs::remove_file(&tmp);
                if second_error.kind() == std::io::ErrorKind::Other {
                    error
                } else {
                    second_error
                }
            })
        }
        Err(error) => {
            let _ = std::fs::remove_file(&tmp);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut st = InstallState {
            version: 1,
            installed_at: "2026-06-25T00:00:00Z".into(),
            ..Default::default()
        };
        st.codex.managed_hook_events = vec!["SessionStart".into(), "Stop".into()];
        st.codex.writable_root_added = Some("C:/Users/PC/.ai-handoff/ipc".into());
        st.codex.created_sandbox_table = true;
        save(dir.path(), &st).unwrap();
        let back = load(dir.path());
        assert_eq!(back, st);
    }

    #[test]
    fn missing_state_is_default_v1() {
        let dir = tempfile::tempdir().unwrap();
        let st = load(dir.path());
        assert_eq!(st.version, 1);
        assert!(st.codex.managed_hook_events.is_empty());
    }

    #[test]
    fn save_overwrites_existing_state() {
        let dir = tempfile::tempdir().unwrap();
        let first = InstallState {
            installed_at: "first".into(),
            ..Default::default()
        };
        save(dir.path(), &first).unwrap();

        let second = InstallState {
            installed_at: "second".into(),
            scheduled_task: Some("AI Handoff".into()),
            ..Default::default()
        };
        save(dir.path(), &second).unwrap();

        assert_eq!(load(dir.path()), second);
    }

    #[test]
    fn roundtrips_hkcu_autostart_state() {
        let dir = tempfile::tempdir().unwrap();
        let st = InstallState {
            installed_at: "with-autostart".into(),
            autostart: Some(AutostartState::new(AutostartKind::HkcuRun, "AI Handoff")),
            ..Default::default()
        };
        save(dir.path(), &st).unwrap();
        assert_eq!(load(dir.path()), st);
    }

    #[test]
    fn roundtrips_launcher_state() {
        let dir = tempfile::tempdir().unwrap();
        let st = InstallState {
            installed_at: "with-launcher".into(),
            launcher: Some(LauncherState {
                path: Some("C:\\Users\\PC\\.ai-handoff\\bin\\aho.cmd".into()),
                path_dir_added: Some("C:\\Users\\PC\\.ai-handoff\\bin".into()),
            }),
            ..Default::default()
        };
        save(dir.path(), &st).unwrap();
        assert_eq!(load(dir.path()), st);
    }
}
