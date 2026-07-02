use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

use ai_handoff_core::{
    install::{apply_uninstall, state, targets_for, InstallTargets},
    paths,
};
use anyhow::{bail, Context};

use super::autostart::delete_autostart;

pub use super::autostart::{delete_hkcu_run_argv, delete_task_argv};

/// What `ai-handoff uninstall` removes.
#[derive(Clone, Copy, Debug, Default)]
pub struct UninstallOptions {
    pub keep_store: bool,
    pub purge_store: bool,
    /// Also remove the desktop GUI app (GUI removal always includes TUI/CLI).
    pub gui: bool,
    /// Everything: GUI + TUI/CLI + local store/log/ipc data.
    pub all: bool,
    /// Skip interactive confirmation prompts (used by the TUI/GUI buttons).
    pub yes: bool,
}

pub fn run(opts: UninstallOptions) -> anyhow::Result<i32> {
    let base_dirs = directories::BaseDirs::new().context("could not determine user home")?;
    let exe = std::env::current_exe().context("could not determine current executable")?;
    let targets = targets_for(
        base_dirs.home_dir(),
        &paths::home(),
        &paths::ipc_dir(),
        &exe,
    );
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let code = run_with_targets(&targets, opts, &mut stdin.lock(), &mut stdout.lock(), true)?;
    // The GUI app (NSIS uninstaller + WebView/Start Menu leftovers) goes last so
    // the config/hook cleanup above is already durable if it fails.
    if opts.gui || opts.all {
        uninstall_gui_app(&mut std::io::stdout().lock())?;
    }
    // Best-effort: drop the managed binary itself. A running Windows exe cannot
    // delete itself, so this schedules a detached delayed delete.
    schedule_managed_binary_removal(&targets.home, &exe);
    Ok(code)
}

pub fn run_with_targets(
    targets: &InstallTargets,
    opts: UninstallOptions,
    input: &mut dyn Read,
    out: &mut dyn Write,
    delete_task: bool,
) -> anyhow::Result<i32> {
    if opts.keep_store && opts.purge_store {
        bail!("--keep-store and --purge-store cannot be used together");
    }
    let purge_store = opts.purge_store || opts.all;
    if opts.keep_store && opts.all {
        bail!("--keep-store and --all cannot be used together");
    }
    let include_gui = opts.gui || opts.all;

    let st = state::load(&targets.home);
    apply_uninstall(targets, &st)?;

    if delete_task {
        delete_autostart(&st)?;
    }
    super::launcher::remove_aho_launcher(&st)?;
    let removed_leftovers = cleanup_stale_managed_paths_for_targets(targets, include_gui)?;
    if !removed_leftovers.is_empty() {
        writeln!(
            out,
            "Removed {} managed leftover path(s).",
            removed_leftovers.len()
        )?;
    }
    purge_file(&state::state_path(&targets.home))?;

    if purge_store {
        if opts.yes
            || confirm(
                input,
                out,
                "Delete local AI Handoff store/log/ipc data? [y/N] ",
            )?
        {
            purge_local_data(targets)?;
            writeln!(out, "Local AI Handoff data purged.")?;
        } else {
            writeln!(out, "Purge cancelled; local data kept.")?;
        }
    } else {
        writeln!(out, "Local AI Handoff store/logs kept.")?;
    }

    writeln!(out, "Uninstall complete.")?;
    Ok(0)
}

fn confirm(input: &mut dyn Read, out: &mut dyn Write, prompt: &str) -> anyhow::Result<bool> {
    write!(out, "{prompt}")?;
    out.flush()?;
    let mut answer = String::new();
    input.read_to_string(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn purge_dir(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn purge_file(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn purge_local_data(targets: &InstallTargets) -> std::io::Result<()> {
    purge_dir(&targets.home.join("store"))?;
    purge_dir(&targets.ipc_dir)?;
    purge_dir(&targets.home.join("logs"))?;
    purge_file(&state::state_path(&targets.home))?;
    Ok(())
}

fn cleanup_stale_managed_paths_for_targets(
    targets: &InstallTargets,
    include_gui: bool,
) -> std::io::Result<Vec<PathBuf>> {
    let Some(user_home) = user_home_from_targets(targets) else {
        return Ok(Vec::new());
    };
    let local_app_data = env_path("LOCALAPPDATA");
    let roaming_app_data = env_path("APPDATA");
    let temp_dir = std::env::temp_dir();
    cleanup_stale_managed_paths(
        &user_home,
        local_app_data.as_deref(),
        roaming_app_data.as_deref(),
        &temp_dir,
        include_gui,
    )
}

fn user_home_from_targets(targets: &InstallTargets) -> Option<PathBuf> {
    targets
        .codex_config
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}

fn cleanup_stale_managed_paths(
    user_home: &Path,
    local_app_data: Option<&Path>,
    roaming_app_data: Option<&Path>,
    temp_dir: &Path,
    include_gui: bool,
) -> std::io::Result<Vec<PathBuf>> {
    let mut candidates = vec![
        user_home.join(".codex/plugins/cache/claude-codex-auto-handoff"),
        user_home.join(".codex/.tmp/marketplaces/claude-codex-auto-handoff"),
        user_home.join(".claude/plugins/data/ai-handoff-skills-dir"),
    ];

    if include_gui {
        if let Some(root) = local_app_data {
            candidates.push(root.join("com.lumisia.aihandoff"));
        }
        if let Some(root) = roaming_app_data {
            candidates.push(root.join("Microsoft/Windows/Start Menu/Programs/AI Handoff"));
        }
    }

    let mut removed = Vec::new();
    for path in candidates {
        remove_managed_dir_if_exists(&path, &mut removed)?;
    }

    // The Start Menu search shortcut we drop next to NSIS's "AI Handoff.lnk"
    // so the app is findable by typing "aho" (a file, not a directory).
    if include_gui {
        if let Some(root) = roaming_app_data {
            let aho = root.join("Microsoft/Windows/Start Menu/Programs/aho.lnk");
            remove_managed_file_if_exists(&aho, &mut removed)?;
        }
    }

    if temp_dir.is_dir() {
        for entry in std::fs::read_dir(temp_dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() && name.to_str().is_some_and(|name| name.starts_with("ah-bi-")) {
                remove_managed_dir_if_exists(&path, &mut removed)?;
            }
        }
    }

    Ok(removed)
}

fn remove_managed_dir_if_exists(path: &Path, removed: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)?;
        removed.push(path.to_path_buf());
    }
    Ok(())
}

fn remove_managed_file_if_exists(path: &Path, removed: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
        removed.push(path.to_path_buf());
    }
    Ok(())
}

/// Run the desktop GUI's own uninstaller (Windows NSIS, silent). The desktop
/// app quits itself right after launching us, so wait briefly before the
/// uninstaller tries to delete its files.
fn uninstall_gui_app(out: &mut dyn Write) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        let Some((uninstaller, args)) = windows_gui_uninstall_command() else {
            writeln!(out, "Desktop GUI not found; skipping GUI uninstall.")?;
            return Ok(());
        };
        std::thread::sleep(std::time::Duration::from_millis(1500));
        writeln!(out, "Running GUI uninstaller: {}", uninstaller.display())?;
        let status = std::process::Command::new(&uninstaller)
            .args(&args)
            .status()
            .with_context(|| format!("could not run {}", uninstaller.display()))?;
        if status.success() {
            writeln!(out, "Desktop GUI uninstalled.")?;
        } else {
            writeln!(out, "GUI uninstaller exited with {status}.")?;
        }
    }
    #[cfg(not(windows))]
    {
        writeln!(out, "GUI uninstall is Windows-only for now; skipping.")?;
    }
    Ok(())
}

/// Locate the NSIS uninstaller for the "AI Handoff" desktop app: the registry
/// uninstall entry first (HKCU then HKLM), then the default per-user install
/// directory. Always runs silent (`/S`).
#[cfg(windows)]
fn windows_gui_uninstall_command() -> Option<(PathBuf, Vec<String>)> {
    for hive in ["HKCU", "HKLM"] {
        let key = format!(r"{hive}\Software\Microsoft\Windows\CurrentVersion\Uninstall\AI Handoff");
        for value in ["QuietUninstallString", "UninstallString"] {
            if let Some(raw) = reg_query_value(&key, value) {
                if let Some(exe) = parse_command_program(&raw) {
                    if exe.is_file() {
                        return Some((exe, vec!["/S".into()]));
                    }
                }
            }
        }
    }
    let local = std::env::var_os("LOCALAPPDATA")?;
    let exe = PathBuf::from(local)
        .join("AI Handoff")
        .join("uninstall.exe");
    exe.is_file().then(|| (exe, vec!["/S".into()]))
}

#[cfg(windows)]
fn reg_query_value(key: &str, value: &str) -> Option<String> {
    let output = std::process::Command::new("reg")
        .args(["query", key, "/v", value])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix(value)
            .and_then(|rest| rest.split_once("REG_"))
            .and_then(|(_, rest)| rest.split_once(' '))
            .map(|(_, data)| data.trim().to_string())
    })
}

/// The program path out of a registry command line, e.g.
/// `"C:\x\uninstall.exe" /S` or `C:\x\uninstall.exe`.
fn parse_command_program(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix('"') {
        return rest.split('"').next().map(PathBuf::from);
    }
    // Unquoted: everything up to a ` /` switch (paths may contain spaces).
    let end = raw.find(" /").unwrap_or(raw.len());
    Some(PathBuf::from(raw[..end].trim()))
}

/// Delete the installed `~/.ai-handoff/bin` binary after this process exits.
/// Only fires when the running exe actually lives in the managed bin dir (a
/// dev build run from `target/` is left alone). Best effort by design.
fn schedule_managed_binary_removal(ai_home: &Path, exe: &Path) {
    let bin_dir = ai_home.join("bin");
    if !exe.starts_with(&bin_dir) {
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // A running exe cannot delete itself; a detached cmd waits for this
        // process to exit, then removes the binary and the (then empty) bin dir.
        let script = format!(
            "ping -n 3 127.0.0.1 > nul & del /f /q \"{exe}\" \"{exe}.old\" 2> nul & rmdir \"{bin}\" 2> nul",
            exe = exe.display(),
            bin = bin_dir.display(),
        );
        // raw_arg: std's default quoting escapes the embedded quotes in a way
        // cmd.exe does not understand.
        let mut command = std::process::Command::new("cmd");
        command.arg("/C");
        command.raw_arg(&script);
        let _ = command
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(not(windows))]
    {
        // Unlinking a running binary is fine on unix.
        let _ = std::fs::remove_file(exe);
        let _ = std::fs::remove_dir(&bin_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_task_argv_targets_ai_handoff_task() {
        assert_eq!(
            delete_task_argv(),
            vec!["/Delete", "/TN", "AI Handoff", "/F"]
        );
    }

    #[test]
    fn uninstall_removes_recorded_launcher_cmd() {
        let dir = tempfile::tempdir().unwrap();
        let user_home = dir.path();
        let ai_home = user_home.join("ai-home");
        let bin = ai_home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let launcher = bin.join("aho.cmd");
        std::fs::write(&launcher, "@echo off\r\n").unwrap();
        state::save(
            &ai_home,
            &state::InstallState {
                launcher: Some(state::LauncherState {
                    path: Some(launcher.to_string_lossy().into_owned()),
                    path_dir_added: None,
                }),
                ..Default::default()
            },
        )
        .unwrap();
        let targets = targets_for(
            user_home,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        let mut input: &[u8] = b"";
        let mut output = Vec::new();

        let code = run_with_targets(
            &targets,
            UninstallOptions::default(),
            &mut input,
            &mut output,
            false,
        )
        .unwrap();

        assert_eq!(code, 0);
        assert!(!launcher.exists());
    }

    #[test]
    fn all_implies_purge_and_yes_skips_the_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let user_home = dir.path();
        let ai_home = user_home.join("ai-home");
        std::fs::create_dir_all(ai_home.join("store")).unwrap();
        std::fs::write(ai_home.join("store/capsule.json"), "{}").unwrap();
        let targets = targets_for(
            user_home,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        // No stdin available: --yes must make --all non-interactive.
        let mut input: &[u8] = b"";
        let mut output = Vec::new();

        let code = run_with_targets(
            &targets,
            UninstallOptions {
                all: true,
                yes: true,
                ..Default::default()
            },
            &mut input,
            &mut output,
            false,
        )
        .unwrap();

        assert_eq!(code, 0);
        assert!(!ai_home.join("store").exists());
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("Local AI Handoff data purged."));
    }

    #[test]
    fn parse_command_program_handles_quoted_and_bare_paths() {
        assert_eq!(
            parse_command_program(r#""C:\Users\x\AppData\Local\AI Handoff\uninstall.exe" /S"#),
            Some(PathBuf::from(
                r"C:\Users\x\AppData\Local\AI Handoff\uninstall.exe"
            ))
        );
        assert_eq!(
            parse_command_program(r"C:\Apps\AI Handoff\uninstall.exe /S"),
            Some(PathBuf::from(r"C:\Apps\AI Handoff\uninstall.exe"))
        );
        assert_eq!(
            parse_command_program(r"C:\Apps\AI Handoff\uninstall.exe"),
            Some(PathBuf::from(r"C:\Apps\AI Handoff\uninstall.exe"))
        );
    }

    #[test]
    fn stale_cleanup_removes_known_windows_leftovers_but_preserves_user_configs() {
        let dir = tempfile::tempdir().unwrap();
        let user_home = dir.path().join("home");
        let local_app_data = dir.path().join("local");
        let roaming_app_data = dir.path().join("roaming");
        let temp_dir = dir.path().join("temp");

        let stale_dirs = [
            user_home.join(".codex/plugins/cache/claude-codex-auto-handoff"),
            user_home.join(".codex/.tmp/marketplaces/claude-codex-auto-handoff"),
            user_home.join(".claude/plugins/data/ai-handoff-skills-dir"),
            local_app_data.join("com.lumisia.aihandoff"),
            roaming_app_data.join("Microsoft/Windows/Start Menu/Programs/AI Handoff"),
            temp_dir.join("ah-bi-test123"),
        ];
        for path in &stale_dirs {
            std::fs::create_dir_all(path).unwrap();
            std::fs::write(path.join("marker.txt"), "owned").unwrap();
        }
        // The top-level "aho" search shortcut is a file, not a directory.
        let aho_lnk = roaming_app_data.join("Microsoft/Windows/Start Menu/Programs/aho.lnk");
        std::fs::create_dir_all(aho_lnk.parent().unwrap()).unwrap();
        std::fs::write(&aho_lnk, "lnk").unwrap();
        let untouched_temp = temp_dir.join("other-tool");
        std::fs::create_dir_all(&untouched_temp).unwrap();

        let configs = [
            user_home.join(".claude/settings.json"),
            user_home.join(".claude.json"),
            user_home.join(".codex/config.toml"),
        ];
        for path in &configs {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "user data").unwrap();
        }

        let removed = cleanup_stale_managed_paths(
            &user_home,
            Some(&local_app_data),
            Some(&roaming_app_data),
            &temp_dir,
            true,
        )
        .unwrap();

        for path in &stale_dirs {
            assert!(!path.exists(), "stale path was not removed: {path:?}");
        }
        assert!(!aho_lnk.exists(), "aho.lnk shortcut was not removed");
        for path in &configs {
            assert!(path.exists(), "config file must be preserved: {path:?}");
            assert_eq!(std::fs::read_to_string(path).unwrap(), "user data");
        }
        assert!(untouched_temp.exists());
        assert_eq!(removed.len(), stale_dirs.len() + 1);
    }

    #[test]
    fn stale_cleanup_can_leave_gui_leftovers_for_tui_only_uninstall() {
        let dir = tempfile::tempdir().unwrap();
        let user_home = dir.path().join("home");
        let local_app_data = dir.path().join("local");
        let roaming_app_data = dir.path().join("roaming");
        let temp_dir = dir.path().join("temp");
        let gui_paths = [
            local_app_data.join("com.lumisia.aihandoff"),
            roaming_app_data.join("Microsoft/Windows/Start Menu/Programs/AI Handoff"),
        ];
        for path in &gui_paths {
            std::fs::create_dir_all(path).unwrap();
        }
        let codex_cache = user_home.join(".codex/plugins/cache/claude-codex-auto-handoff");
        std::fs::create_dir_all(&codex_cache).unwrap();

        let removed = cleanup_stale_managed_paths(
            &user_home,
            Some(&local_app_data),
            Some(&roaming_app_data),
            &temp_dir,
            false,
        )
        .unwrap();

        assert!(!codex_cache.exists());
        for path in &gui_paths {
            assert!(
                path.exists(),
                "GUI path should remain in TUI-only mode: {path:?}"
            );
        }
        assert_eq!(removed, vec![codex_cache]);
    }
}
