//! Interactive account **add** / **launch** via the official vendor CLIs.
//!
//! We never reimplement OAuth. The official CLI performs the login (opening a
//! browser or showing a device code) and writes the credential; we then capture
//! it into the vault. The TUI is suspended while these run because the CLIs take
//! over the terminal.
//!
//! - add: `codex login` / `claude auth login` into a temp profile home, then
//!   `account::capture_login` copies the result into a vault slot.
//! - launch: run the agent with `CODEX_HOME` / `CLAUDE_CONFIG_DIR` pointed at a
//!   saved slot, so it uses that account for this session only.

use std::path::PathBuf;
use std::process::Command;

use ai_handoff_core::account::{self, Agent};

/// Run the official login for `agent` into a temp profile home, then capture the
/// resulting credential into a vault slot. Returns the new slot label.
pub fn add_account(agent: Agent) -> Result<String, String> {
    let home = temp_login_home(agent)?;
    // Codex: force file-backed credentials so the login result is a file we can
    // capture (otherwise it may land in the OS keyring).
    if agent == Agent::Codex {
        let _ = std::fs::write(
            home.join("config.toml"),
            "cli_auth_credentials_store = \"file\"\n",
        );
    }
    let (program, args, var) = login_command(agent);
    let exe = account::which(program)
        .ok_or_else(|| format!("`{program}` not found on PATH — install it first"))?;
    let status = Command::new(exe)
        .args(args)
        .env(var, &home)
        .status()
        .map_err(|e| format!("could not launch `{program}`: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&home);
        return Err(format!("`{program}` login did not complete"));
    }
    let result = account::capture_login(agent, &home, "official-cli-login").map_err(|e| e.to_string());
    let _ = std::fs::remove_dir_all(&home);
    result
}

/// Launch `agent` under a saved slot's profile home (this session only).
pub fn launch(agent: Agent, label: &str) -> Result<(), String> {
    let (var, home) = account::profile_env(agent, label);
    let _ = std::fs::create_dir_all(&home);
    let program = agent_program(agent);
    let exe = account::which(program)
        .ok_or_else(|| format!("`{program}` not found on PATH — install it first"))?;
    Command::new(exe)
        .env(var, &home)
        .status()
        .map_err(|e| format!("could not launch `{program}`: {e}"))?;
    Ok(())
}

/// `(program, args, profile-home env var)` for the official login.
fn login_command(agent: Agent) -> (&'static str, &'static [&'static str], &'static str) {
    match agent {
        Agent::Codex => ("codex", &["login"], "CODEX_HOME"),
        Agent::Claude => ("claude", &["auth", "login"], "CLAUDE_CONFIG_DIR"),
    }
}

fn agent_program(agent: Agent) -> &'static str {
    match agent {
        Agent::Codex => "codex",
        Agent::Claude => "claude",
    }
}

fn temp_login_home(agent: Agent) -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = ai_handoff_core::paths::home()
        .join("tmp")
        .join("login")
        .join(agent_program(agent))
        .join(stamp.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("temp dir: {e}"))?;
    Ok(dir)
}
