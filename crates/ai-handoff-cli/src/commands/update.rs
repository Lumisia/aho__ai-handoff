//! `ai-handoff update` — re-run the official installer script so the CLI/TUI
//! (and optionally the desktop GUI) are replaced with the latest stable
//! GitHub Release. The script owns download, checksum verification, and the
//! locked-binary swap, so this works identically for every user's machine.

use std::process::Command;

const REPO_RAW_BASE: &str =
    "https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts";

pub fn run(gui: bool) -> anyhow::Result<i32> {
    println!(
        "Updating AI Handoff (current v{}) to the latest release...",
        env!("CARGO_PKG_VERSION")
    );

    #[cfg(windows)]
    {
        let script = powershell_update_command(gui);
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .status()?;
        Ok(status.code().unwrap_or(1))
    }

    #[cfg(not(windows))]
    {
        if gui {
            eprintln!("note: GUI update is Windows-only for now; updating the CLI/TUI.");
        }
        let status = Command::new("sh")
            .args(["-c", &shell_update_command()])
            .status()?;
        Ok(status.code().unwrap_or(1))
    }
}

/// The PowerShell one-liner that fetches and runs `scripts/install.ps1`.
fn powershell_update_command(gui: bool) -> String {
    let mut command = format!(
        "Set-ExecutionPolicy Bypass -Scope Process -Force; \
         & ([scriptblock]::Create((irm '{REPO_RAW_BASE}/install.ps1'))) -Yes"
    );
    if gui {
        command.push_str(" -WithGui");
    }
    command
}

#[cfg(not(windows))]
fn shell_update_command() -> String {
    format!("curl -fsSL {REPO_RAW_BASE}/install.sh | sh")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_command_targets_official_installer() {
        let cmd = powershell_update_command(false);
        assert!(cmd.contains("install.ps1"));
        assert!(cmd.contains("-Yes"));
        assert!(!cmd.contains("-WithGui"));
    }

    #[test]
    fn powershell_command_adds_gui_flag() {
        let cmd = powershell_update_command(true);
        assert!(cmd.ends_with("-WithGui"));
    }
}
