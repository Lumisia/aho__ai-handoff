use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct InstallTargets {
    pub home: PathBuf,
    pub ipc_dir: PathBuf,
    pub exe: PathBuf,
    pub codex_hooks: PathBuf,
    pub codex_config: PathBuf,
    pub claude_settings: PathBuf,
    /// Claude plugin bundle dir: `~/.claude/skills/ai-handoff` — dropping the
    /// bundle here makes Claude auto-load it as `ai-handoff@skills-dir`.
    pub claude_plugin_dir: PathBuf,
    /// Codex plugin bundle dir: `~/.agents/plugins/ai-handoff`.
    pub codex_plugin_dir: PathBuf,
    /// Codex personal marketplace manifest: `~/.agents/plugins/marketplace.json`
    /// (auto-discovered by Codex; an entry here registers the local bundle).
    pub agents_marketplace: PathBuf,
}

pub fn targets_for(user_home: &Path, ai_home: &Path, ipc_dir: &Path, exe: &Path) -> InstallTargets {
    let claude_settings = user_home.join(".claude").join("settings.json");
    // Anchor the Claude plugin dir to the settings dir so a relocated `.claude`
    // (e.g. an MSIX redirect) keeps the bundle next to its settings.
    let claude_plugin_dir = claude_settings
        .parent()
        .map(|p| p.join("skills").join("ai-handoff"))
        .unwrap_or_else(|| user_home.join(".claude").join("skills").join("ai-handoff"));
    InstallTargets {
        home: ai_home.to_path_buf(),
        ipc_dir: ipc_dir.to_path_buf(),
        exe: exe.to_path_buf(),
        codex_hooks: user_home.join(".codex").join("hooks.json"),
        codex_config: user_home.join(".codex").join("config.toml"),
        claude_settings,
        claude_plugin_dir,
        codex_plugin_dir: user_home.join(".agents").join("plugins").join("ai-handoff"),
        agents_marketplace: user_home
            .join(".agents")
            .join("plugins")
            .join("marketplace.json"),
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
        assert_eq!(t.claude_plugin_dir, uh.join(".claude/skills/ai-handoff"));
        assert_eq!(t.codex_plugin_dir, uh.join(".agents/plugins/ai-handoff"));
        assert_eq!(
            t.agents_marketplace,
            uh.join(".agents/plugins/marketplace.json")
        );
        let p = detect_agents(&t);
        assert!(p.codex);
        assert!(!p.claude);
    }
}
