use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, Status, VERSION},
};
use anyhow::Context;
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::time::Duration;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub fn run(json_output: bool, fix: bool) -> anyhow::Result<i32> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    Ok(run_io_fix(json_output, fix, &mut out))
}

/// Diagnostics-only entry point (kept so existing callers/tests read the
/// same signature semantics: no repairs, just the report).
pub fn run_io(json_output: bool, out: &mut dyn Write) -> i32 {
    run_io_fix(json_output, false, out)
}

pub fn run_io_fix(json_output: bool, fix: bool, out: &mut dyn Write) -> i32 {
    // --fix runs the repairs FIRST so the printed report shows the state the
    // user actually ends up with.
    let fixes = if fix { apply_fixes() } else { Vec::new() };
    let req = Request {
        version: VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: "ping".to_string(),
        agent: "codex".to_string(),
        event: "ping".to_string(),
        received_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        cwd: std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        session_id: None,
        turn_id: None,
        raw_hook_input: json!({}),
        client: ClientInfo {
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            platform: std::env::consts::OS.to_string(),
        },
    };
    let resp = send(
        &req,
        &ClientConfig {
            request_timeout: Duration::from_millis(120),
            poll_interval: Duration::from_millis(5),
            ..Default::default()
        },
    );
    let daemon = if resp.status == Status::Ok {
        "reachable"
    } else {
        "unreachable"
    };
    // The RUNNING daemon's own store-write health, reported on the ping. This
    // is distinct from this process's probe below: a sandboxed daemon can hold
    // the singleton lock while every consume/checkpoint write fails, and only
    // the daemon itself can see that. Older daemons don't report the flag.
    let daemon_store_writable = resp.hook_stdout.get("store_writable").cloned();
    let ipc_permissions = permission_report(ai_handoff_core::secure_fs::private_dir_status(
        &ai_handoff_core::paths::ipc_dir(),
    ));
    // Store write probe from THIS process (the doctor may itself be sandboxed;
    // the probe fails the same way capsule writes would).
    let store_permissions = store_report(&ai_handoff_core::paths::store_dir());
    // The root ACL alone missed the real failure mode: hardened
    // requests/responses subdirs lock sandboxed agents out while the root
    // still reads "private". Check inheritance AND actually try to write.
    let ipc_requests = ipc_subdir_report(&ai_handoff_core::paths::requests_dir());
    let ipc_responses = ipc_subdir_report(&ai_handoff_core::paths::responses_dir());

    // Per-agent plugin install state, read from the recorded install-state.
    let st = ai_handoff_core::install::state::load(&ai_handoff_core::paths::home());
    let claude_plugin = claude_plugin_state(&st.claude.plugin);
    let codex_plugin = codex_plugin_state(&st.codex.plugin);
    let host_inspection = crate::host_launcher::inspect(st.host_launcher.as_ref());
    let host_launcher = json!({
        "recorded": host_inspection.recorded,
        "registered": host_inspection.registered,
        "fixed_action": host_inspection.action_matches,
        "executable_present": host_inspection.executable_present,
        "id": st.host_launcher.as_ref().map(|host| host.id.as_str()),
        "kind": st.host_launcher.as_ref().map(|host| format!("{:?}", host.kind)),
        "error": host_inspection.error,
    });
    let mut next_steps = next_steps(daemon == "reachable", fix, &claude_plugin, &codex_plugin);
    if daemon_store_writable.as_ref().and_then(|v| v.as_bool()) == Some(false) {
        next_steps.push(
            "the running daemon cannot WRITE the capsule store (sandboxed daemon?) — \
             stop it and run `ai-handoff daemon run` outside the sandbox"
                .to_string(),
        );
    }

    let report = json!({
        "daemon": daemon,
        "daemon_store_writable": daemon_store_writable,
        "home": ai_handoff_core::paths::home().to_string_lossy(),
        "ipc": ai_handoff_core::paths::ipc_dir().to_string_lossy(),
        "ipc_permissions": ipc_permissions,
        "ipc_requests": ipc_requests,
        "ipc_responses": ipc_responses,
        "store_permissions": store_permissions,
        "host_launcher": host_launcher,
        "plugin": {
            "claude": claude_plugin,
            "codex": codex_plugin,
        },
        "fixes": fixes,
        "next_steps": next_steps,
    });

    if json_output {
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        let _ = writeln!(out, "daemon: {daemon}");
        let _ = writeln!(
            out,
            "ipc permissions: {} ({})",
            report["ipc_permissions"]["status"]
                .as_str()
                .unwrap_or("unknown"),
            report["ipc_permissions"]["message"].as_str().unwrap_or("")
        );
        let _ = writeln!(
            out,
            "ipc requests: {} ({})",
            report["ipc_requests"]["status"]
                .as_str()
                .unwrap_or("unknown"),
            report["ipc_requests"]["message"].as_str().unwrap_or("")
        );
        let _ = writeln!(
            out,
            "ipc responses: {} ({})",
            report["ipc_responses"]["status"]
                .as_str()
                .unwrap_or("unknown"),
            report["ipc_responses"]["message"].as_str().unwrap_or("")
        );
        let _ = writeln!(
            out,
            "store permissions: {} ({})",
            report["store_permissions"]["status"]
                .as_str()
                .unwrap_or("unknown"),
            report["store_permissions"]["message"]
                .as_str()
                .unwrap_or("")
        );
        let _ = writeln!(
            out,
            "daemon store write: {}",
            match report["daemon_store_writable"].as_bool() {
                Some(true) => "ok",
                Some(false) => "DENIED",
                None => "unknown (daemon unreachable or older version)",
            }
        );
        let _ = writeln!(
            out,
            "host launcher: executable={}, recorded={}, registered={}, fixed_action={}",
            report["host_launcher"]["executable_present"]
                .as_bool()
                .unwrap_or(false),
            report["host_launcher"]["recorded"]
                .as_bool()
                .unwrap_or(false),
            report["host_launcher"]["registered"]
                .as_bool()
                .unwrap_or(false),
            match report["host_launcher"]["fixed_action"].as_bool() {
                Some(true) => "yes",
                Some(false) => "NO",
                None => "unknown",
            }
        );
        let _ = writeln!(
            out,
            "claude plugin: {}/{}",
            mark(
                claude_plugin["installed"].as_bool().unwrap_or(false),
                "installed",
                "not installed"
            ),
            mark(
                claude_plugin["enabled"].as_bool().unwrap_or(false),
                "enabled",
                "not enabled"
            )
        );
        let _ = writeln!(
            out,
            "codex plugin: {}/{}/{}",
            mark(
                codex_plugin["installed"].as_bool().unwrap_or(false),
                "installed",
                "not installed"
            ),
            mark(
                codex_plugin["enabled"].as_bool().unwrap_or(false),
                "enabled",
                "not enabled"
            ),
            mark(
                codex_plugin["trusted"].as_bool().unwrap_or(false),
                "trusted",
                "trust needed"
            )
        );
        for fixed in report["fixes"].as_array().into_iter().flatten() {
            let _ = writeln!(out, "fix: {}", fixed.as_str().unwrap_or(""));
        }
        for step in report["next_steps"].as_array().into_iter().flatten() {
            let _ = writeln!(out, "next step: {}", step.as_str().unwrap_or(""));
        }
    }
    0
}

/// The `--fix` pass: repair what a local command can safely repair, and
/// describe each action taken. Anything needing the vendor CLI or user
/// interaction stays in `next_steps` instead.
fn apply_fixes() -> Vec<String> {
    let mut fixes = Vec::new();

    // 1. Runtime tree: recreate/re-harden home, IPC (root private, subdirs
    //    inheriting), store, and logs — the same repairs daemon startup and
    //    `install --yes` perform.
    match repair_runtime_dirs() {
        Ok(()) => fixes.push(
            "repaired runtime directories (home, ipc requests/responses/dead-letter, store, logs)"
                .to_string(),
        ),
        Err(error) => fixes.push(format!("runtime directory repair FAILED: {error}")),
    }

    if let Some(host_fix) = repair_host_launcher() {
        fixes.push(host_fix);
    }

    // 2. Daemon: spawn one when unreachable and wait until it answers.
    if crate::daemon_supply::ping_daemon(Duration::from_millis(150)) {
        return fixes;
    }
    match crate::daemon_supply::ensure_daemon() {
        Ok(outcome) if outcome.ready => fixes.push(format!(
            "started the daemon via {:?} (it was unreachable)",
            outcome.strategy
        )),
        Ok(outcome) => fixes.push(format!(
            "daemon start FAILED — {}",
            outcome.errors.join("; ")
        )),
        Err(error) => fixes.push(format!("daemon start FAILED — {error}")),
    }
    fixes
}

fn host_launcher_needs_repair(
    inspection: &crate::host_launcher::LauncherInspection,
    managed_codex_install: bool,
) -> bool {
    managed_codex_install
        && (!inspection.executable_present
            || !inspection.recorded
            || !inspection.registered
            || inspection.action_matches != Some(true))
}

#[cfg(windows)]
fn repair_host_launcher() -> Option<String> {
    let home = ai_handoff_core::paths::home();
    let mut state = ai_handoff_core::install::state::load(&home);
    let managed_codex_install = state.host_launcher.is_some()
        || state.codex.plugin.is_some()
        || state.codex.hooks_file.is_some()
        || state.codex.writable_root_added.is_some();
    let inspection = crate::host_launcher::inspect(state.host_launcher.as_ref());
    if !host_launcher_needs_repair(&inspection, managed_codex_install) {
        return None;
    }

    let result = (|| -> anyhow::Result<()> {
        let cli = std::env::current_exe().context("resolve current executable")?;
        let host_exe = crate::host_launcher::resolve_host_executable(&cli)?;
        let host_state = crate::host_launcher::install(&host_exe, &home)?;
        state.host_launcher = Some(host_state);
        ai_handoff_core::install::state::save(&home, &state)?;
        Ok(())
    })();
    Some(match result {
        Ok(()) => "re-registered the on-demand host launcher".to_string(),
        Err(error) => format!("host launcher repair FAILED: {error}"),
    })
}

#[cfg(not(windows))]
fn repair_host_launcher() -> Option<String> {
    None
}

/// Mirror of the daemon's `ensure_runtime_dirs`, callable without a daemon.
fn repair_runtime_dirs() -> std::io::Result<()> {
    use ai_handoff_core::{paths, secure_fs};
    secure_fs::ensure_private_dir(&paths::home())?;
    secure_fs::ensure_private_dir(&paths::ipc_dir())?;
    secure_fs::ensure_inherited_subdir(&paths::requests_dir())?;
    secure_fs::ensure_inherited_subdir(&paths::responses_dir())?;
    secure_fs::ensure_inherited_subdir(&paths::dead_letter_dir())?;
    secure_fs::ensure_private_dir(&paths::store_dir())?;
    secure_fs::ensure_private_dir(&paths::logs_dir())?;
    secure_fs::touch_private_file(&paths::logs_dir().join("daemon.log"))
}

/// Actionable follow-ups the user must do themselves (UX: a first-run doctor
/// should say exactly what to do next, especially for the Codex trust step,
/// which no command can perform on the user's behalf).
fn next_steps(
    daemon_reachable: bool,
    fix_ran: bool,
    claude_plugin: &serde_json::Value,
    codex_plugin: &serde_json::Value,
) -> Vec<String> {
    let mut steps = Vec::new();
    let installed = |plugin: &serde_json::Value| plugin["installed"].as_bool().unwrap_or(false);
    if !installed(claude_plugin) || !installed(codex_plugin) {
        steps.push(
            "plugin hooks are not installed for every agent — run `ai-handoff install --yes`"
                .to_string(),
        );
    }
    if installed(codex_plugin) && !codex_plugin["trusted"].as_bool().unwrap_or(false) {
        steps.push(
            "Codex hooks are not trusted yet: open Codex, run /hooks, select each ai-handoff \
             entry and choose Trust, then re-run `ai-handoff doctor`"
                .to_string(),
        );
    }
    if !daemon_reachable && !fix_ran {
        steps.push(
            "daemon unreachable — run `ai-handoff doctor --fix` or `ai-handoff daemon run`"
                .to_string(),
        );
    }
    steps
}

fn mark(ok: bool, yes: &'static str, no: &'static str) -> &'static str {
    if ok {
        yes
    } else {
        no
    }
}

/// Combined health of one IPC subdir: ACL-inheritance state plus an actual
/// write probe. The probe is what catches the sandbox case — when doctor runs
/// inside an agent sandbox (the handoff-doctor skill), a broken ACL makes the
/// probe fail exactly like the hooks do.
fn ipc_subdir_report(dir: &Path) -> serde_json::Value {
    let mut report = ai_handoff_core::secure_fs::inherited_subdir_status(dir);
    if !matches!(
        report.status,
        ai_handoff_core::secure_fs::PermissionStatus::Missing
            | ai_handoff_core::secure_fs::PermissionStatus::Error
    ) {
        if let Err(error) = probe_write(dir) {
            report = ai_handoff_core::secure_fs::PermissionReport {
                status: ai_handoff_core::secure_fs::PermissionStatus::Error,
                message: format!("write test failed: {error}"),
            };
        }
    }
    permission_report(report)
}

fn probe_write(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".ai-handoff-doctor-{}.tmp", std::process::id()));
    std::fs::write(&probe, b"probe").map_err(|error| error.to_string())?;
    std::fs::remove_file(&probe).map_err(|error| error.to_string())?;
    Ok(())
}

/// Capsule-store health: ACL state plus a real write probe, mirroring the
/// daemon's startup preflight. Catches "peek works but consume can never
/// write" before a handoff silently degrades.
fn store_report(dir: &Path) -> serde_json::Value {
    let mut report = ai_handoff_core::secure_fs::private_dir_status(dir);
    if !matches!(
        report.status,
        ai_handoff_core::secure_fs::PermissionStatus::Missing
            | ai_handoff_core::secure_fs::PermissionStatus::Error
    ) {
        if let Err(error) = probe_write(dir) {
            report = ai_handoff_core::secure_fs::PermissionReport {
                status: ai_handoff_core::secure_fs::PermissionStatus::Error,
                message: format!("write test failed: {error}"),
            };
        }
    }
    permission_report(report)
}

fn permission_report(report: ai_handoff_core::secure_fs::PermissionReport) -> serde_json::Value {
    let status = match report.status {
        ai_handoff_core::secure_fs::PermissionStatus::Ok => "ok",
        ai_handoff_core::secure_fs::PermissionStatus::Warning => "warning",
        ai_handoff_core::secure_fs::PermissionStatus::Error => "error",
        ai_handoff_core::secure_fs::PermissionStatus::Missing => "missing",
    };
    json!({
        "status": status,
        "message": report.message,
    })
}

fn claude_plugin_state(rec: &Option<ai_handoff_core::install::PluginRecord>) -> serde_json::Value {
    match rec {
        Some(r) => {
            let installed = Path::new(&r.root)
                .join(".claude-plugin")
                .join("plugin.json")
                .is_file();
            json!({
                "installed": installed,
                "enabled": installed,
                "root": r.root,
            })
        }
        None => json!({
            "installed": false,
            "enabled": false,
        }),
    }
}

fn codex_plugin_state(rec: &Option<ai_handoff_core::install::PluginRecord>) -> serde_json::Value {
    match rec {
        Some(r) => {
            let installed = Path::new(&r.root)
                .join(".codex-plugin")
                .join("plugin.json")
                .is_file();
            let config_text = codex_config_path(r)
                .and_then(|path| std::fs::read_to_string(path).ok())
                .unwrap_or_default();
            let enabled =
                ai_handoff_core::install::duplicate::codex_v2_plugin_enabled(&config_text);
            let trusted =
                ai_handoff_core::install::duplicate::codex_v2_plugin_trusted(&config_text);
            json!({
                "installed": installed,
                "enabled": enabled,
                "trusted": trusted,
                "root": r.root,
            })
        }
        None => json!({
            "installed": false,
            "enabled": false,
            "trusted": false,
        }),
    }
}

fn codex_config_path(rec: &ai_handoff_core::install::PluginRecord) -> Option<PathBuf> {
    let marketplace = rec.marketplace_file.as_ref()?;
    let user_home = Path::new(marketplace).parent()?.parent()?.parent()?;
    Some(user_home.join(".codex").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_launcher::LauncherInspection;

    #[test]
    fn host_launcher_repair_requires_a_managed_codex_install_and_broken_registration() {
        let healthy = LauncherInspection {
            recorded: true,
            registered: true,
            action_matches: Some(true),
            executable_present: true,
            error: None,
        };
        let missing = LauncherInspection {
            recorded: true,
            registered: false,
            action_matches: None,
            executable_present: false,
            error: None,
        };
        let missing_executable = LauncherInspection {
            recorded: true,
            registered: true,
            action_matches: Some(true),
            executable_present: false,
            error: None,
        };

        assert!(!host_launcher_needs_repair(&healthy, true));
        assert!(host_launcher_needs_repair(&missing, true));
        assert!(host_launcher_needs_repair(&missing_executable, true));
        assert!(!host_launcher_needs_repair(&missing, false));
    }
}
