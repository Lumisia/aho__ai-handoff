use std::path::Path;

use ai_handoff_core::install::state::{HostLauncherKind, HostLauncherState};

#[cfg(windows)]
mod windows;

pub const WINDOWS_FOLDER: &str = "AIHandoff";
pub const WINDOWS_TASK: &str = "Daemon";
pub const WINDOWS_TASK_ID: &str = r"\AIHandoff\Daemon";

pub fn host_executable_name() -> &'static str {
    if cfg!(windows) {
        "ai-handoff-host.exe"
    } else {
        "ai-handoff-host"
    }
}

pub fn resolve_host_executable(cli_exe: &Path) -> anyhow::Result<std::path::PathBuf> {
    anyhow::ensure!(
        cli_exe.is_absolute(),
        "CLI executable path must be absolute"
    );
    let expected_cli = if cfg!(windows) {
        "ai-handoff.exe"
    } else {
        "ai-handoff"
    };
    let actual_cli = cli_exe
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("CLI executable has no valid file name"))?;
    let name_matches = if cfg!(windows) {
        actual_cli.eq_ignore_ascii_case(expected_cli)
    } else {
        actual_cli == expected_cli
    };
    anyhow::ensure!(
        name_matches,
        "expected managed CLI executable named {expected_cli}, got {actual_cli}"
    );
    let host = cli_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("CLI executable has no parent directory"))?
        .join(host_executable_name());
    anyhow::ensure!(
        host.is_file(),
        "managed background host is missing: {}",
        host.display()
    );
    Ok(host)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherProbe {
    Ready,
    Missing,
    WrongPlatform,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LauncherInspection {
    pub recorded: bool,
    pub registered: bool,
    pub action_matches: Option<bool>,
    pub executable_present: bool,
    pub error: Option<String>,
}

pub fn task_action_arguments(home: &Path) -> String {
    format!(r#"--home "{}""#, home.display())
}

#[cfg(windows)]
pub fn install(exe: &Path, home: &Path) -> anyhow::Result<HostLauncherState> {
    install_with(exe, home, windows::install)
}

#[cfg(not(windows))]
pub fn install(_exe: &Path, _home: &Path) -> anyhow::Result<HostLauncherState> {
    anyhow::bail!("on-demand host launcher is not implemented for this OS")
}

#[cfg(windows)]
pub fn launch(state: Option<&HostLauncherState>) -> anyhow::Result<()> {
    launch_with(state, windows::launch)
}

#[cfg(not(windows))]
pub fn launch(state: Option<&HostLauncherState>) -> anyhow::Result<()> {
    launch_with(state, || {
        anyhow::bail!("on-demand host launcher is not implemented for this OS")
    })
}

#[cfg(windows)]
pub fn remove(state: Option<&HostLauncherState>) -> anyhow::Result<()> {
    remove_with(state, windows::remove)
}

#[cfg(not(windows))]
pub fn remove(state: Option<&HostLauncherState>) -> anyhow::Result<()> {
    remove_with(state, || {
        if state.is_none() {
            Ok(())
        } else {
            anyhow::bail!("on-demand host launcher is not implemented for this OS")
        }
    })
}

#[cfg(windows)]
fn expected_kind() -> HostLauncherKind {
    HostLauncherKind::WindowsTaskScheduler
}

#[cfg(target_os = "macos")]
fn expected_kind() -> HostLauncherKind {
    HostLauncherKind::MacLaunchAgent
}

#[cfg(all(unix, not(target_os = "macos")))]
fn expected_kind() -> HostLauncherKind {
    HostLauncherKind::LinuxSystemdUser
}

#[cfg(windows)]
fn install_with<F>(exe: &Path, home: &Path, backend: F) -> anyhow::Result<HostLauncherState>
where
    F: FnOnce(&Path, &Path) -> anyhow::Result<()>,
{
    anyhow::ensure!(
        exe.file_name().and_then(|name| name.to_str()) == Some(host_executable_name()),
        "host launcher must use {}",
        host_executable_name()
    );
    backend(exe, home)?;
    Ok(HostLauncherState {
        kind: HostLauncherKind::WindowsTaskScheduler,
        id: WINDOWS_TASK_ID.to_string(),
        artifact_paths: vec![exe.to_string_lossy().into_owned()],
    })
}

fn launch_with<F>(state: Option<&HostLauncherState>, backend: F) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<()>,
{
    let state = state.ok_or_else(|| anyhow::anyhow!("host launcher is not installed"))?;
    anyhow::ensure!(
        state.kind == expected_kind(),
        "recorded host launcher does not match this OS"
    );
    backend()
}

fn remove_with<F>(state: Option<&HostLauncherState>, backend: F) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<()>,
{
    if let Some(state) = state {
        anyhow::ensure!(
            state.kind == expected_kind(),
            "recorded host launcher does not match this OS"
        );
    }
    backend()
}

pub fn probe(state: Option<&HostLauncherState>) -> LauncherProbe {
    match state {
        None => LauncherProbe::Missing,
        Some(state) if state.kind != expected_kind() => LauncherProbe::WrongPlatform,
        Some(_) => LauncherProbe::Ready,
    }
}

#[cfg(windows)]
pub fn inspect(state: Option<&HostLauncherState>) -> LauncherInspection {
    let recorded = probe(state) == LauncherProbe::Ready;
    let expected_home = ai_handoff_core::paths::home();
    let expected_exe = expected_home.join("bin").join(host_executable_name());
    let executable_present = expected_exe.is_file();
    match windows::inspect() {
        Ok(Some(xml)) => LauncherInspection {
            recorded,
            registered: true,
            action_matches: Some(action_xml_matches(&xml, &expected_exe, &expected_home)),
            executable_present,
            error: None,
        },
        Ok(None) => LauncherInspection {
            recorded,
            registered: false,
            action_matches: None,
            executable_present,
            error: None,
        },
        Err(error) => LauncherInspection {
            recorded,
            registered: false,
            action_matches: None,
            executable_present,
            error: Some(error.to_string()),
        },
    }
}

#[cfg(not(windows))]
pub fn inspect(state: Option<&HostLauncherState>) -> LauncherInspection {
    let host = ai_handoff_core::paths::home()
        .join("bin")
        .join(host_executable_name());
    LauncherInspection {
        recorded: probe(state) == LauncherProbe::Ready,
        registered: false,
        action_matches: None,
        executable_present: host.is_file(),
        error: Some("native host launcher inspection is not implemented for this OS".into()),
    }
}

fn action_xml_matches(xml: &str, expected_exe: &Path, expected_home: &Path) -> bool {
    let Some(command) = xml_tag_text(xml, "command") else {
        return false;
    };
    let Some(arguments) = xml_tag_text(xml, "arguments") else {
        return false;
    };
    command.eq_ignore_ascii_case(expected_exe.to_string_lossy().trim())
        && arguments.eq_ignore_ascii_case(&task_action_arguments(expected_home))
        && !arguments.to_ascii_lowercase().contains(" hook ")
}

fn xml_tag_text(xml: &str, tag: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = lower.find(&open)?.saturating_add(open.len());
    let end = start.saturating_add(lower.get(start..)?.find(&close)?);
    let text = xml.get(start..end)?.trim();
    (!text.is_empty()).then(|| decode_xml_text(text))
}

fn decode_xml_text(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_managed_host_as_an_exact_cli_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join(if cfg!(windows) {
            "ai-handoff.exe"
        } else {
            "ai-handoff"
        });
        let host = dir.path().join(host_executable_name());
        std::fs::write(&host, b"host").unwrap();

        assert_eq!(resolve_host_executable(&cli).unwrap(), host);
    }

    #[test]
    fn host_resolution_rejects_missing_or_noncanonical_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join(if cfg!(windows) {
            "ai-handoff.exe"
        } else {
            "ai-handoff"
        });
        let missing = resolve_host_executable(&cli).unwrap_err();
        assert!(missing.to_string().contains(host_executable_name()));

        let other = dir
            .path()
            .join(if cfg!(windows) { "other.exe" } else { "other" });
        std::fs::write(dir.path().join(host_executable_name()), b"host").unwrap();
        assert!(resolve_host_executable(&other).is_err());
    }

    #[test]
    fn windows_task_identity_is_stable() {
        assert_eq!(WINDOWS_FOLDER, "AIHandoff");
        assert_eq!(WINDOWS_TASK, "Daemon");
        assert_eq!(WINDOWS_TASK_ID, r"\AIHandoff\Daemon");
    }

    #[test]
    fn task_action_uses_fixed_daemon_home_arguments() {
        let args = task_action_arguments(std::path::Path::new(r"C:\Users\me\.ai-handoff"));

        assert_eq!(args, r#"--home "C:\Users\me\.ai-handoff""#);
        assert!(!args.contains("daemon"));
        assert!(!args.contains("hook"));
    }

    #[cfg(windows)]
    #[test]
    fn install_records_windows_task_state_after_backend_success() {
        let exe = std::path::Path::new(r"C:\Users\me\.ai-handoff\bin\ai-handoff-host.exe");
        let home = std::path::Path::new(r"C:\Users\me\.ai-handoff");
        let mut called = false;

        let state = install_with(exe, home, |actual_exe, actual_home| {
            called = true;
            assert_eq!(actual_exe, exe);
            assert_eq!(actual_home, home);
            Ok(())
        })
        .unwrap();

        assert!(called);
        assert_eq!(
            state.kind,
            ai_handoff_core::install::state::HostLauncherKind::WindowsTaskScheduler
        );
        assert_eq!(state.id, WINDOWS_TASK_ID);
        assert_eq!(state.artifact_paths, vec![exe.to_string_lossy()]);
    }

    #[cfg(windows)]
    #[test]
    fn launch_rejects_a_different_launcher_kind() {
        let state = ai_handoff_core::install::state::HostLauncherState {
            kind: ai_handoff_core::install::state::HostLauncherKind::MacLaunchAgent,
            id: "com.lumisia.ai-handoff".into(),
            artifact_paths: Vec::new(),
        };
        let mut called = false;

        let error = launch_with(Some(&state), || {
            called = true;
            Ok(())
        })
        .unwrap_err();

        assert!(!called);
        assert!(error.to_string().contains("does not match this OS"));
    }

    #[test]
    fn remove_without_recorded_state_still_runs_orphan_cleanup() {
        let mut called = false;

        remove_with(None, || {
            called = true;
            Ok(())
        })
        .unwrap();

        assert!(called);
    }

    #[test]
    fn probe_reports_missing_without_install_state() {
        assert_eq!(probe(None), LauncherProbe::Missing);
    }

    #[cfg(windows)]
    #[test]
    fn probe_reports_wrong_platform_for_non_windows_state() {
        let state = ai_handoff_core::install::state::HostLauncherState {
            kind: ai_handoff_core::install::state::HostLauncherKind::LinuxSystemdUser,
            id: "ai-handoff.service".into(),
            artifact_paths: Vec::new(),
        };

        assert_eq!(probe(Some(&state)), LauncherProbe::WrongPlatform);
    }

    #[test]
    fn public_lifecycle_api_has_stable_signatures() {
        let _: fn(
            &std::path::Path,
            &std::path::Path,
        ) -> anyhow::Result<ai_handoff_core::install::state::HostLauncherState> = install;
        let _: fn(
            Option<&ai_handoff_core::install::state::HostLauncherState>,
        ) -> anyhow::Result<()> = launch;
        let _: fn(
            Option<&ai_handoff_core::install::state::HostLauncherState>,
        ) -> anyhow::Result<()> = remove;
    }

    #[test]
    fn fixed_action_xml_requires_daemon_home_and_rejects_hook_input() {
        let valid = r#"<Task><Actions><Exec><Command>C:\Users\me\.ai-handoff\bin\ai-handoff-host.exe</Command><Arguments>--home &quot;C:\Users\me\.ai-handoff&quot;</Arguments></Exec></Actions></Task>"#;
        let unsafe_hook = r#"<Task><Actions><Exec><Command>ai-handoff.exe</Command><Arguments>hook session-start</Arguments></Exec></Actions></Task>"#;
        let stale_home = r#"<Task><Actions><Exec><Command>C:\Users\me\.ai-handoff\bin\ai-handoff-host.exe</Command><Arguments>--home &quot;C:\Users\other\.ai-handoff&quot;</Arguments></Exec></Actions></Task>"#;
        let wrong_exe = r#"<Task><Actions><Exec><Command>C:\Temp\other.exe</Command><Arguments>daemon run --home &quot;C:\Users\me\.ai-handoff&quot;</Arguments></Exec></Actions></Task>"#;

        let expected_exe = std::path::Path::new(r"C:\Users\me\.ai-handoff\bin\ai-handoff-host.exe");
        let expected_home = std::path::Path::new(r"C:\Users\me\.ai-handoff");
        assert!(action_xml_matches(valid, expected_exe, expected_home));
        assert!(!action_xml_matches(
            unsafe_hook,
            expected_exe,
            expected_home
        ));
        assert!(!action_xml_matches(stale_home, expected_exe, expected_home));
        assert!(!action_xml_matches(wrong_exe, expected_exe, expected_home));
    }
}
