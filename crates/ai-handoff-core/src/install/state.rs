use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct FileMod {
    pub path: String,
    pub backup: Option<String>,
}

/// Record of a generated, installed plugin bundle for one agent.
///
/// `root` is the absolute bundle directory; `files` are the relative paths
/// written under it (for surgical uninstall). `marketplace_file`, when set,
/// is the marketplace registration file Task 4 records.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct PluginRecord {
    pub root: String,
    pub files: Vec<String>,
    #[serde(default)]
    pub marketplace_file: Option<String>,
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
    #[serde(default)]
    pub plugin: Option<PluginRecord>,
    #[serde(default)]
    pub handoff_skill: Option<PluginRecord>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct ClaudeStatuslineState {
    pub previous: Option<serde_json::Value>,
    pub installed_command: String,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct ClaudeState {
    pub settings_file: Option<FileMod>,
    pub managed_hook_events: Vec<String>,
    #[serde(default)]
    pub statusline: Option<ClaudeStatuslineState>,
    #[serde(default)]
    pub plugin: Option<PluginRecord>,
    #[serde(default)]
    pub handoff_skill: Option<PluginRecord>,
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostLauncherKind {
    WindowsTaskScheduler,
    MacLaunchAgent,
    LinuxSystemdUser,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HostLauncherState {
    pub kind: HostLauncherKind,
    pub id: String,
    #[serde(default)]
    pub artifact_paths: Vec<String>,
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
    #[serde(default)]
    pub host_launcher: Option<HostLauncherState>,
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
            host_launcher: None,
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
    crate::secure_fs::ensure_private_dir(ai_home)?;
    let p = state_path(ai_home);
    let tmp = p.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(st)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    crate::secure_fs::write_private_atomic(&p, &tmp, &json)
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
    fn roundtrips_claude_statusline_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut st = InstallState {
            installed_at: "with-statusline".into(),
            ..Default::default()
        };
        st.claude.statusline = Some(ClaudeStatuslineState {
            previous: Some(serde_json::json!({"type":"command","command":"my-prompt"})),
            installed_command: "\"C:\\p\\ai-handoff.exe\" statusline".into(),
        });
        save(dir.path(), &st).unwrap();
        assert_eq!(load(dir.path()), st);
    }

    #[test]
    fn roundtrips_plugin_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut st = InstallState {
            installed_at: "with-plugin".into(),
            ..Default::default()
        };
        st.claude.plugin = Some(PluginRecord {
            root: "C:\\Users\\PC\\.ai-handoff\\plugins\\claude\\ai-handoff".into(),
            files: vec![
                ".claude-plugin/plugin.json".into(),
                "hooks/hooks.json".into(),
                "skills/handoff-checkpoint/SKILL.md".into(),
            ],
            marketplace_file: None,
        });
        st.claude.handoff_skill = Some(PluginRecord {
            root: "C:\\Users\\PC\\.claude\\skills".into(),
            files: vec!["handoff-checkpoint/SKILL.md".into()],
            marketplace_file: None,
        });
        st.codex.plugin = Some(PluginRecord {
            root: "C:\\Users\\PC\\.ai-handoff\\plugins\\codex\\ai-handoff".into(),
            files: vec![
                ".codex-plugin/plugin.json".into(),
                "hooks/hooks.json".into(),
            ],
            marketplace_file: Some("C:\\Users\\PC\\.codex\\marketplace.json".into()),
        });
        st.codex.handoff_skill = Some(PluginRecord {
            root: "C:\\Users\\PC\\.agents\\skills\\handoff".into(),
            files: vec!["SKILL.md".into()],
            marketplace_file: None,
        });
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

    #[test]
    fn roundtrips_windows_host_launcher_state() {
        let dir = tempfile::tempdir().unwrap();
        let st = InstallState {
            installed_at: "with-host-launcher".into(),
            host_launcher: Some(HostLauncherState {
                kind: HostLauncherKind::WindowsTaskScheduler,
                id: r"\AIHandoff\Daemon".into(),
                artifact_paths: Vec::new(),
            }),
            ..Default::default()
        };

        save(dir.path(), &st).unwrap();

        assert_eq!(load(dir.path()), st);
    }

    #[test]
    fn old_state_without_host_launcher_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            state_path(dir.path()),
            br#"{"version":1,"installed_at":"","codex":{},"claude":{}}"#,
        )
        .unwrap();

        assert_eq!(load(dir.path()).host_launcher, None);
    }
}
