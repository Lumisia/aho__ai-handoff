use std::io::{Read, Write};

use ai_handoff_core::{
    install::{
        apply_install, detect_agents, diff, plan_install, state, targets_for, AgentPresence,
        AutostartState, InstallTargets,
    },
    paths,
};
use anyhow::{bail, Context};

use super::autostart::{delete_autostart, delete_autostart_state, register_autostart, TASK_NAME};

pub use super::autostart::{hkcu_run_argv, scheduled_task_argv};

type AutostartRegistrar<'a> = &'a mut dyn FnMut(&str) -> anyhow::Result<AutostartState>;
type LauncherInstaller<'a> = &'a mut dyn FnMut(
    &std::path::Path,
    Option<&std::path::Path>,
) -> anyhow::Result<state::LauncherState>;

struct InstallSideEffects<'a> {
    register_autostart: Option<AutostartRegistrar<'a>>,
    install_launcher: Option<LauncherInstaller<'a>>,
}

pub fn run(dry_run: bool, yes: bool, agents: Option<Vec<String>>) -> anyhow::Result<i32> {
    let base_dirs = directories::BaseDirs::new().context("could not determine user home")?;
    let exe = std::env::current_exe().context("could not determine current executable")?;
    let targets = targets_for(
        base_dirs.home_dir(),
        &paths::home(),
        &paths::ipc_dir(),
        &exe,
    );
    // Autostart-on-logon is opt-in via `[autostart] enabled` in config.toml;
    // it defaults to disabled, so a fresh install registers no logon task.
    let autostart_enabled = ai_handoff_core::config::load().autostart.enabled;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    run_with_targets(
        &targets,
        dry_run,
        yes,
        agents,
        &mut stdin.lock(),
        &mut stdout.lock(),
        autostart_enabled,
    )
}

pub fn run_with_targets(
    targets: &InstallTargets,
    dry_run: bool,
    yes: bool,
    agents: Option<Vec<String>>,
    input: &mut dyn Read,
    out: &mut dyn Write,
    autostart_enabled: bool,
) -> anyhow::Result<i32> {
    // Toggle off: when autostart is disabled (the default), remove any logon
    // registration a previous install left behind, so flipping the config key
    // back to false and re-installing actually turns autostart off.
    if !dry_run && !autostart_enabled {
        let prior = state::load(&targets.home);
        if prior.autostart.is_some() || prior.scheduled_task.is_some() {
            let _ = delete_autostart(&prior);
        }
    }

    let mut register = |exe: &str| register_autostart(exe);
    let register = if autostart_enabled {
        Some(&mut register as AutostartRegistrar<'_>)
    } else {
        None
    };
    // The launcher (aho.cmd + PATH) is the CLI shim, independent of the logon
    // autostart, so it always installs on a real (non-dry-run) install.
    let mut install_launcher =
        |home: &std::path::Path, gui: Option<&std::path::Path>| {
            super::launcher::install_aho_launcher(home, gui)
        };
    let install_launcher = Some(&mut install_launcher as LauncherInstaller<'_>);
    run_with_targets_impl(
        targets,
        dry_run,
        yes,
        agents,
        input,
        out,
        InstallSideEffects {
            register_autostart: register,
            install_launcher,
        },
    )
}

fn run_with_targets_impl(
    targets: &InstallTargets,
    dry_run: bool,
    yes: bool,
    agents: Option<Vec<String>>,
    input: &mut dyn Read,
    out: &mut dyn Write,
    side_effects: InstallSideEffects<'_>,
) -> anyhow::Result<i32> {
    let detected = detect_agents(targets);
    let selected = filter_agents(detected, agents.as_deref())?;
    if !selected.codex && !selected.claude {
        writeln!(out, "No matching Codex or Claude config directory found.")?;
        return Ok(1);
    }

    let now = chrono::Utc::now();
    let plan = plan_install(targets, &selected, now)?;
    writeln!(
        out,
        "{}",
        diff::render(&plan.file_plans, "ai-handoff install")
    )?;
    for finding in &plan.duplicates {
        writeln!(out, "Warning ({}): {}", finding.agent, finding.detail)?;
    }

    if dry_run {
        writeln!(out, "Dry run only; no files were changed.")?;
        return Ok(0);
    }

    if !yes && !confirm(input, out, "Apply these changes? [y/N] ")? {
        writeln!(out, "Install cancelled.")?;
        return Ok(1);
    }

    let autostart = if let Some(register) = side_effects.register_autostart {
        Some(register(&targets.exe.to_string_lossy())?)
    } else {
        None
    };
    let launcher = if let Some(install_launcher) = side_effects.install_launcher {
        Some(install_launcher(&targets.home, None)?)
    } else {
        None
    };

    let mut st = match apply_install(targets, &selected, now) {
        Ok(st) => st,
        Err(error) => {
            if let Some(autostart) = &autostart {
                let _ = delete_autostart_state(autostart);
            }
            if let Some(launcher) = launcher {
                let cleanup = state::InstallState {
                    launcher: Some(launcher),
                    ..Default::default()
                };
                let _ = super::launcher::remove_aho_launcher(&cleanup);
            }
            return Err(error);
        }
    };
    if let Some(autostart) = autostart.clone() {
        if autostart.kind == ai_handoff_core::install::AutostartKind::ScheduledTask {
            st.scheduled_task = Some(TASK_NAME.to_string());
        }
        st.autostart = Some(autostart.clone());
    }
    st.launcher = launcher;
    if st.autostart.is_some() || st.launcher.is_some() {
        state::save(&targets.home, &st)?;
    }

    if selected.codex {
        writeln!(
            out,
            "Codex hooks installed. Open Codex /hooks and trust the new hooks."
        )?;
    }
    if selected.claude {
        writeln!(out, "Claude hooks installed.")?;
    }
    if let Some(autostart) = &st.autostart {
        writeln!(out, "Autostart: {:?}", autostart.kind)?;
    } else {
        writeln!(
            out,
            "Autostart: disabled (set [autostart] enabled = true in ~/.ai-handoff/config.toml to run the daemon at logon)"
        )?;
    }
    if let Some(launcher) = &st.launcher {
        if let Some(path) = &launcher.path {
            writeln!(out, "Launcher: {path}")?;
        }
    }
    Ok(0)
}

fn filter_agents(
    detected: AgentPresence,
    filters: Option<&[String]>,
) -> anyhow::Result<AgentPresence> {
    let Some(filters) = filters else {
        return Ok(detected);
    };
    if filters.is_empty() {
        return Ok(detected);
    }

    let mut selected = AgentPresence {
        codex: false,
        claude: false,
    };
    for filter in filters {
        match filter.trim().to_ascii_lowercase().as_str() {
            "codex" => selected.codex = detected.codex,
            "claude" | "claude-code" | "claude_code" => selected.claude = detected.claude,
            "all" => return Ok(detected),
            other => bail!("unknown agent filter: {other}"),
        }
    }
    Ok(selected)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_failure_aborts_before_config_writes() {
        let dir = tempfile::tempdir().unwrap();
        let user_home = dir.path();
        std::fs::create_dir_all(user_home.join(".codex")).unwrap();
        std::fs::create_dir_all(user_home.join(".claude")).unwrap();
        let codex_config = "sandbox_mode = \"workspace-write\"\n";
        let claude_settings = r#"{"model":"opus"}"#;
        std::fs::write(user_home.join(".codex/config.toml"), codex_config).unwrap();
        std::fs::write(user_home.join(".claude/settings.json"), claude_settings).unwrap();

        let ai_home = user_home.join("ai-home");
        let targets = targets_for(
            user_home,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        let mut input: &[u8] = b"";
        let mut output = Vec::new();
        let mut fail_register = |_exe: &str| anyhow::bail!("simulated autostart failure");

        let err = run_with_targets_impl(
            &targets,
            false,
            true,
            None,
            &mut input,
            &mut output,
            InstallSideEffects {
                register_autostart: Some(&mut fail_register),
                install_launcher: None,
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("simulated autostart"));
        assert_eq!(
            std::fs::read_to_string(user_home.join(".codex/config.toml")).unwrap(),
            codex_config
        );
        assert_eq!(
            std::fs::read_to_string(user_home.join(".claude/settings.json")).unwrap(),
            claude_settings
        );
        assert!(!user_home.join(".codex/hooks.json").exists());
        assert!(!ai_home.join("install-state.json").exists());
    }

    #[test]
    fn hkcu_autostart_success_is_recorded_in_install_state() {
        let dir = tempfile::tempdir().unwrap();
        let user_home = dir.path();
        std::fs::create_dir_all(user_home.join(".codex")).unwrap();
        std::fs::create_dir_all(user_home.join(".claude")).unwrap();
        std::fs::write(
            user_home.join(".codex/config.toml"),
            "sandbox_mode = \"workspace-write\"\n",
        )
        .unwrap();
        std::fs::write(
            user_home.join(".claude/settings.json"),
            r#"{"model":"opus"}"#,
        )
        .unwrap();

        let ai_home = user_home.join("ai-home");
        let targets = targets_for(
            user_home,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        let mut input: &[u8] = b"";
        let mut output = Vec::new();
        let mut register = |_exe: &str| {
            Ok(AutostartState::new(
                ai_handoff_core::install::AutostartKind::HkcuRun,
                TASK_NAME,
            ))
        };

        let code = run_with_targets_impl(
            &targets,
            false,
            true,
            None,
            &mut input,
            &mut output,
            InstallSideEffects {
                register_autostart: Some(&mut register),
                install_launcher: None,
            },
        )
        .unwrap();

        assert_eq!(code, 0);
        let st = state::load(&ai_home);
        assert_eq!(
            st.autostart.unwrap().kind,
            ai_handoff_core::install::AutostartKind::HkcuRun
        );
        assert!(st.scheduled_task.is_none());
    }

    #[test]
    fn launcher_success_is_recorded_in_install_state() {
        let dir = tempfile::tempdir().unwrap();
        let user_home = dir.path();
        std::fs::create_dir_all(user_home.join(".codex")).unwrap();
        std::fs::create_dir_all(user_home.join(".claude")).unwrap();
        std::fs::write(
            user_home.join(".codex/config.toml"),
            "sandbox_mode = \"workspace-write\"\n",
        )
        .unwrap();
        std::fs::write(
            user_home.join(".claude/settings.json"),
            r#"{"model":"opus"}"#,
        )
        .unwrap();

        let ai_home = user_home.join("ai-home");
        let targets = targets_for(
            user_home,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        let mut input: &[u8] = b"";
        let mut output = Vec::new();
        let mut register = |_exe: &str| {
            Ok(AutostartState::new(
                ai_handoff_core::install::AutostartKind::HkcuRun,
                TASK_NAME,
            ))
        };
        let mut launcher = |home: &std::path::Path, _gui: Option<&std::path::Path>| {
            Ok(state::LauncherState {
                path: Some(home.join("bin").join("aho.cmd").to_string_lossy().into_owned()),
                path_dir_added: Some(home.join("bin").to_string_lossy().into_owned()),
            })
        };

        let code = run_with_targets_impl(
            &targets,
            false,
            true,
            None,
            &mut input,
            &mut output,
            InstallSideEffects {
                register_autostart: Some(&mut register),
                install_launcher: Some(&mut launcher),
            },
        )
        .unwrap();

        assert_eq!(code, 0);
        let st = state::load(&ai_home);
        assert!(st
            .launcher
            .as_ref()
            .and_then(|launcher| launcher.path.as_ref())
            .unwrap()
            .ends_with("aho.cmd"));
        assert!(st
            .launcher
            .as_ref()
            .and_then(|launcher| launcher.path_dir_added.as_ref())
            .unwrap()
            .ends_with("bin"));
    }

    #[test]
    fn autostart_disabled_keeps_launcher_and_records_no_autostart() {
        let dir = tempfile::tempdir().unwrap();
        let user_home = dir.path();
        std::fs::create_dir_all(user_home.join(".codex")).unwrap();
        std::fs::create_dir_all(user_home.join(".claude")).unwrap();
        std::fs::write(
            user_home.join(".codex/config.toml"),
            "sandbox_mode = \"workspace-write\"\n",
        )
        .unwrap();
        std::fs::write(user_home.join(".claude/settings.json"), r#"{"model":"opus"}"#).unwrap();

        let ai_home = user_home.join("ai-home");
        let targets = targets_for(
            user_home,
            &ai_home,
            &ai_home.join("ipc"),
            std::path::Path::new("C:/p/ai-handoff.exe"),
        );
        let mut input: &[u8] = b"";
        let mut output = Vec::new();
        let mut launcher = |home: &std::path::Path, _gui: Option<&std::path::Path>| {
            Ok(state::LauncherState {
                path: Some(home.join("bin").join("aho.cmd").to_string_lossy().into_owned()),
                path_dir_added: Some(home.join("bin").to_string_lossy().into_owned()),
            })
        };

        // register_autostart: None models a disabled-autostart install.
        let code = run_with_targets_impl(
            &targets,
            false,
            true,
            None,
            &mut input,
            &mut output,
            InstallSideEffects {
                register_autostart: None,
                install_launcher: Some(&mut launcher),
            },
        )
        .unwrap();

        assert_eq!(code, 0);
        let st = state::load(&ai_home);
        assert!(st.autostart.is_none());
        assert!(st.scheduled_task.is_none());
        assert!(st.launcher.is_some());
        let out = String::from_utf8(output).unwrap();
        assert!(out.contains("Autostart: disabled"));
    }
}
