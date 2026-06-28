//! Account **add** / **launch** via the official vendor CLIs, each in its own
//! new terminal window (so the TUI keeps running in its own console instead of
//! being taken over).
//!
//! - add: open a window running `codex login` / `claude auth login` into a temp
//!   profile home; the caller polls for the credential and captures it
//!   (`account::login_complete` / `account::capture_login`).
//! - launch: open a window running the agent with `CODEX_HOME` /
//!   `CLAUDE_CONFIG_DIR` pointed at a saved slot (this session only).
//!
//! We never reimplement OAuth — the official CLI owns the flow and writes the
//! credential; we only capture the resulting file.

use std::path::{Path, PathBuf};
use std::process::Command;

use ai_handoff_core::account::{self, Agent};

/// Open a new window running the official login into a fresh temp profile home.
/// Returns that home; the caller polls it for the captured credential.
pub fn spawn_add_window(agent: Agent) -> Result<PathBuf, String> {
    let home = temp_login_home(agent)?;
    // Codex: force file-backed credentials so the result is a file we can read.
    if agent == Agent::Codex {
        let _ = std::fs::write(
            home.join("config.toml"),
            "cli_auth_credentials_store = \"file\"\n",
        );
    }
    let (program, args, var) = login_command(agent);
    if account::which(program).is_none() {
        return Err(format!("`{program}` not found on PATH — install it first"));
    }
    spawn_window(var, &home, program, args)?;
    Ok(home)
}

/// Open a new window running the agent under a saved slot's profile home.
pub fn spawn_launch_window(agent: Agent, label: &str) -> Result<(), String> {
    let (var, home) = account::profile_env(agent, label);
    let _ = std::fs::create_dir_all(&home);
    let program = agent_program(agent);
    if account::which(program).is_none() {
        return Err(format!("`{program}` not found on PATH — install it first"));
    }
    spawn_window(var, &home, program, &[])
}

/// Spawn `program args` in a NEW terminal window with `var=home` in its env, not
/// blocking the caller.
fn spawn_window(var: &str, home: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    let mut command_line = program.to_string();
    for a in args {
        command_line.push(' ');
        command_line.push_str(a);
    }
    #[cfg(windows)]
    {
        // `cmd /C start "" cmd /K "<cmd>"` opens a fresh console that resolves
        // PATH/PATHEXT (so .cmd shims run) and stays open after the command.
        Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K", &command_line])
            .env(var, home)
            .spawn()
            .map_err(|e| format!("could not open a new window: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        // No portable "new terminal" primitive; run detached in the background.
        let _ = command_line;
        Command::new(program)
            .args(args)
            .env(var, home)
            .spawn()
            .map_err(|e| format!("could not launch `{program}`: {e}"))?;
    }
    Ok(())
}

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
