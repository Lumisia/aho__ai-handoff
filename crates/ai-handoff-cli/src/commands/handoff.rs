//! `ai-handoff handoff` — explicitly consume the pending capsule for this
//! project (the /handoff skill's backend). Prints the daemon's hook-style JSON
//! so skills can read `hookSpecificOutput.additionalContext`; `{}` means no
//! pending capsule targets this agent (capsules addressed to other agents are
//! listed in diagnostics and can be claimed with --force or `retarget`).

use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, Status, VERSION},
};
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::io::Write;
use std::time::Duration;

pub fn run(agent: &str, peek: bool, force: bool, id: Option<&str>) -> anyhow::Result<i32> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    Ok(run_io(agent, peek, force, id, &mut out, true))
}

pub fn run_io(
    agent: &str,
    peek: bool,
    force: bool,
    id: Option<&str>,
    out: &mut dyn Write,
    autostart_daemon: bool,
) -> i32 {
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut raw = json!({ "cwd": cwd, "force": force });
    // Explicit claim: name exactly one capsule (wins over --force).
    if let Some(capsule_id) = id {
        raw["capsule_id"] = json!(capsule_id);
    }

    let req = Request {
        version: VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        // --peek previews the pending capsule without marking it consumed.
        kind: if peek {
            "handoff_peek".to_string()
        } else {
            "handoff_consume".to_string()
        },
        agent: agent.to_string(),
        event: "handoff".to_string(),
        received_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        cwd: cwd.clone(),
        session_id: None,
        turn_id: None,
        raw_hook_input: raw,
        client: ClientInfo {
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            platform: std::env::consts::OS.to_string(),
        },
    };

    let mut resp = send(&req, &ClientConfig::default());
    if autostart_daemon
        && super::hook::daemon_unavailable(&resp)
        && super::hook::start_daemon_logged()
    {
        resp = send(
            &req,
            &ClientConfig {
                request_timeout: Duration::from_millis(2500),
                ..ClientConfig::default()
            },
        );
    }

    for warning in &resp.warnings {
        eprintln!("[ai-handoff] {warning}");
    }
    let text = serde_json::to_string(&resp.hook_stdout).unwrap_or_else(|_| "{}".to_string());
    let _ = writeln!(out, "{text}");
    // Degraded/error means the handoff did NOT happen (unknown --id, capsule
    // already consumed, daemon unreachable) — scripts must not read the bare
    // `{}` as success. "Nothing pending" is still an Ok response and exits 0.
    if resp.status == Status::Ok {
        0
    } else {
        1
    }
}
