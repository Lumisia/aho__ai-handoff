use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct InstallTargets {
    pub home: PathBuf,
    pub ipc_dir: PathBuf,
    pub exe: PathBuf,
    pub codex_hooks: PathBuf,
    pub codex_config: PathBuf,
    pub claude_settings: PathBuf,
}

pub fn targets_for(user_home: &Path, ai_home: &Path, ipc_dir: &Path, exe: &Path) -> InstallTargets {
    InstallTargets {
        home: ai_home.to_path_buf(),
        ipc_dir: ipc_dir.to_path_buf(),
        exe: exe.to_path_buf(),
        codex_hooks: user_home.join(".codex").join("hooks.json"),
        codex_config: user_home.join(".codex").join("config.toml"),
        claude_settings: user_home.join(".claude").join("settings.json"),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AgentPresence {
    pub codex: bool,
    pub claude: bool,
}

pub fn detect_agents(t: &InstallTargets) -> AgentPresence {
    AgentPresence {
        codex: t.codex_config.parent().map(Path::is_dir).unwrap_or(false),
        claude: t
            .claude_settings
            .parent()
            .map(Path::is_dir)
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_present_agents_and_composes_paths() {
        let dir = tempfile::tempdir().unwrap();
        let uh = dir.path();
        fs::create_dir_all(uh.join(".codex")).unwrap();
        // .claude intentionally absent
        let t = targets_for(
            uh,
            &uh.join("ai-home"),
            &uh.join("ai-home/ipc"),
            std::path::Path::new("C:/x/ai-handoff.exe"),
        );
        assert_eq!(t.codex_hooks, uh.join(".codex/hooks.json"));
        assert_eq!(t.codex_config, uh.join(".codex/config.toml"));
        assert_eq!(t.claude_settings, uh.join(".claude/settings.json"));
        let p = detect_agents(&t);
        assert!(p.codex);
        assert!(!p.claude);
    }
}
