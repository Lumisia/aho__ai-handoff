use std::{path::Path, process::Stdio};

use ai_handoff_core::install::state::{AutostartKind, AutostartState, InstallState};
use anyhow::{bail, Context};

pub const TASK_NAME: &str = "AI Handoff";
pub const HKCU_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

pub fn daemon_command(host: &Path, home: &Path) -> String {
    format!("\"{}\" --home \"{}\"", host.display(), home.display())
}

pub fn scheduled_task_argv(host: &Path, home: &Path) -> Vec<String> {
    vec![
        "/Create".into(),
        "/SC".into(),
        "ONLOGON".into(),
        "/TN".into(),
        TASK_NAME.into(),
        "/TR".into(),
        daemon_command(host, home),
        "/RL".into(),
        "LIMITED".into(),
        "/F".into(),
    ]
}

pub fn hkcu_run_argv(host: &Path, home: &Path) -> Vec<String> {
    vec![
        "add".into(),
        HKCU_RUN_KEY.into(),
        "/v".into(),
        TASK_NAME.into(),
        "/t".into(),
        "REG_SZ".into(),
        "/d".into(),
        daemon_command(host, home),
        "/f".into(),
    ]
}

pub fn delete_task_argv() -> Vec<String> {
    vec![
        "/Delete".into(),
        "/TN".into(),
        TASK_NAME.into(),
        "/F".into(),
    ]
}

pub fn delete_hkcu_run_argv() -> Vec<String> {
    vec![
        "delete".into(),
        HKCU_RUN_KEY.into(),
        "/v".into(),
        TASK_NAME.into(),
        "/f".into(),
    ]
}

pub fn register_autostart(host: &Path, home: &Path) -> anyhow::Result<AutostartState> {
    let mut scheduled = |host: &Path, home: &Path| register_scheduled_task(host, home);
    let mut hkcu = |host: &Path, home: &Path| register_hkcu_run(host, home);
    register_autostart_with(host, home, &mut scheduled, &mut hkcu)
}

pub fn register_autostart_with(
    host: &Path,
    home: &Path,
    scheduled: &mut dyn FnMut(&Path, &Path) -> anyhow::Result<()>,
    hkcu: &mut dyn FnMut(&Path, &Path) -> anyhow::Result<()>,
) -> anyhow::Result<AutostartState> {
    match scheduled(host, home) {
        Ok(()) => Ok(AutostartState::new(
            AutostartKind::ScheduledTask,
            TASK_NAME,
        )),
        Err(scheduled_error) => match hkcu(host, home) {
            Ok(()) => Ok(AutostartState::new(AutostartKind::HkcuRun, TASK_NAME)),
            Err(hkcu_error) => bail!(
                "autostart registration failed; scheduled task: {scheduled_error}; HKCU Run: {hkcu_error}"
            ),
        },
    }
}

pub fn delete_autostart(_st: &InstallState) -> anyhow::Result<()> {
    // Best-effort: clear BOTH mechanisms by our fixed name regardless of what
    // install-state recorded. An entry written by an out-of-band path (e.g. a
    // dev build, or one whose state was lost) is an orphan that the old,
    // state-gated delete would skip — and it would still launch at logon. The
    // delete helpers treat "already absent" as success, so this is idempotent.
    delete_scheduled_task()?;
    delete_hkcu_run()?;
    Ok(())
}

pub fn delete_autostart_state(autostart: &AutostartState) -> anyhow::Result<()> {
    match autostart.kind {
        AutostartKind::ScheduledTask => delete_scheduled_task(),
        AutostartKind::HkcuRun => delete_hkcu_run(),
    }
}

fn register_scheduled_task(host: &Path, home: &Path) -> anyhow::Result<()> {
    let status = std::process::Command::new("schtasks")
        .args(scheduled_task_argv(host, home))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to start schtasks /Create")?;
    if !status.success() {
        bail!("schtasks /Create failed with status {status}");
    }
    Ok(())
}

fn register_hkcu_run(host: &Path, home: &Path) -> anyhow::Result<()> {
    let status = std::process::Command::new("reg")
        .args(hkcu_run_argv(host, home))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to start reg add HKCU Run")?;
    if !status.success() {
        bail!("reg add HKCU Run failed with status {status}");
    }
    Ok(())
}

fn delete_scheduled_task() -> anyhow::Result<()> {
    let status = std::process::Command::new("schtasks")
        .args(delete_task_argv())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to start schtasks /Delete")?;
    if !status.success() && scheduled_task_exists()? {
        bail!("schtasks /Delete failed with status {status}");
    }
    Ok(())
}

fn delete_hkcu_run() -> anyhow::Result<()> {
    let status = std::process::Command::new("reg")
        .args(delete_hkcu_run_argv())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to start reg delete HKCU Run")?;
    if !status.success() && hkcu_run_value_exists()? {
        bail!("reg delete HKCU Run failed with status {status}");
    }
    Ok(())
}

fn scheduled_task_exists() -> anyhow::Result<bool> {
    let status = std::process::Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to start schtasks /Query")?;
    Ok(status.success())
}

fn hkcu_run_value_exists() -> anyhow::Result<bool> {
    let status = std::process::Command::new("reg")
        .args(["query", HKCU_RUN_KEY, "/v", TASK_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to start reg query HKCU Run")?;
    Ok(status.success())
}

/// `ai-handoff autostart on|off|status`: apply the OS logon entry and keep the
/// config flag + install-state in sync. The real entry lives in the *user*
/// registry hive, so this must run as the actual installed exe (a sandboxed or
/// MSIX-virtualized process sees a different hive and silently no-ops).
pub fn run_cli(action: crate::AutostartAction) -> anyhow::Result<i32> {
    match action {
        crate::AutostartAction::On => set_autostart(true),
        crate::AutostartAction::Off => set_autostart(false),
        crate::AutostartAction::Status => print_status(),
    }
}

fn set_autostart(enable: bool) -> anyhow::Result<i32> {
    let home = ai_handoff_core::paths::home();
    let config_path = ai_handoff_core::paths::config_path();
    let mut st = ai_handoff_core::install::state::load(&home);

    if enable {
        let cli = std::env::current_exe()
            .context("could not resolve the current executable for autostart")?;
        let host = crate::host_launcher::resolve_host_executable(&cli)?;
        let astate = register_autostart(&host, &home)?;
        st.scheduled_task = if astate.kind == AutostartKind::ScheduledTask {
            Some(TASK_NAME.to_string())
        } else {
            None
        };
        st.autostart = Some(astate);
    } else {
        delete_autostart(&st)?;
        st.autostart = None;
        st.scheduled_task = None;
    }
    let _ = ai_handoff_core::install::state::save(&home, &st);

    // Keep the config flag in sync so the dashboard and daemon agree.
    let mut sink = Vec::new();
    crate::commands::config::run_io(
        crate::ConfigAction::Set {
            key: "autostart.enabled".to_string(),
            value: enable.to_string(),
        },
        &config_path,
        &mut sink,
    )?;

    if enable {
        println!("autostart enabled — the daemon will start at logon");
    } else {
        println!("autostart disabled — removed any logon entry");
    }
    Ok(0)
}

fn print_status() -> anyhow::Result<i32> {
    let cfg = ai_handoff_core::config::load();
    let task = scheduled_task_exists().unwrap_or(false);
    let run = hkcu_run_value_exists().unwrap_or(false);
    println!("config autostart.enabled = {}", cfg.autostart.enabled);
    println!("scheduled task '{TASK_NAME}' present: {task}");
    println!("HKCU Run value '{TASK_NAME}' present: {run}");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduled_task_argv_quotes_host_and_home_on_logon() {
        let argv = scheduled_task_argv(
            std::path::Path::new("C:\\p\\ai-handoff-host.exe"),
            std::path::Path::new("C:\\Users\\me\\.ai-handoff"),
        );
        assert!(argv.contains(&"ONLOGON".to_string()));
        assert!(argv.contains(&"AI Handoff".to_string()));
        assert!(argv.contains(
            &"\"C:\\p\\ai-handoff-host.exe\" --home \"C:\\Users\\me\\.ai-handoff\"".to_string()
        ));
    }

    #[test]
    fn hkcu_run_argv_writes_current_user_run_value() {
        let argv = hkcu_run_argv(
            std::path::Path::new("C:\\p\\ai-handoff-host.exe"),
            std::path::Path::new("C:\\Users\\me\\.ai-handoff"),
        );
        assert!(argv.contains(&"add".to_string()));
        assert!(argv.contains(&HKCU_RUN_KEY.to_string()));
        assert!(argv.contains(&"AI Handoff".to_string()));
        assert!(argv.contains(
            &"\"C:\\p\\ai-handoff-host.exe\" --home \"C:\\Users\\me\\.ai-handoff\"".to_string()
        ));
    }

    #[test]
    fn delete_task_argv_targets_ai_handoff_task() {
        assert_eq!(
            delete_task_argv(),
            vec!["/Delete", "/TN", "AI Handoff", "/F"]
        );
    }

    #[test]
    fn delete_hkcu_run_argv_targets_ai_handoff_value() {
        assert_eq!(
            delete_hkcu_run_argv(),
            vec!["delete", HKCU_RUN_KEY, "/v", "AI Handoff", "/f"]
        );
    }

    #[test]
    fn autostart_prefers_scheduled_task() {
        let mut scheduled_calls = 0;
        let mut hkcu_calls = 0;
        let mut scheduled = |_host: &Path, _home: &Path| {
            scheduled_calls += 1;
            Ok(())
        };
        let mut hkcu = |_host: &Path, _home: &Path| {
            hkcu_calls += 1;
            Ok(())
        };

        let st = register_autostart_with(
            Path::new("C:/p/ai-handoff-host.exe"),
            Path::new("C:/home"),
            &mut scheduled,
            &mut hkcu,
        )
        .unwrap();

        assert_eq!(st.kind, AutostartKind::ScheduledTask);
        assert_eq!(scheduled_calls, 1);
        assert_eq!(hkcu_calls, 0);
    }

    #[test]
    fn autostart_falls_back_to_hkcu_run() {
        let mut scheduled = |_host: &Path, _home: &Path| anyhow::bail!("access denied");
        let mut hkcu_calls = 0;
        let mut hkcu = |_host: &Path, _home: &Path| {
            hkcu_calls += 1;
            Ok(())
        };

        let st = register_autostart_with(
            Path::new("C:/p/ai-handoff-host.exe"),
            Path::new("C:/home"),
            &mut scheduled,
            &mut hkcu,
        )
        .unwrap();

        assert_eq!(st.kind, AutostartKind::HkcuRun);
        assert_eq!(hkcu_calls, 1);
    }

    #[test]
    fn autostart_returns_err_when_both_methods_fail() {
        let mut scheduled = |_host: &Path, _home: &Path| anyhow::bail!("access denied");
        let mut hkcu = |_host: &Path, _home: &Path| anyhow::bail!("registry denied");

        let err = register_autostart_with(
            Path::new("C:/p/ai-handoff-host.exe"),
            Path::new("C:/home"),
            &mut scheduled,
            &mut hkcu,
        )
        .unwrap_err();

        assert!(err.to_string().contains("scheduled task"));
        assert!(err.to_string().contains("HKCU Run"));
    }
}
