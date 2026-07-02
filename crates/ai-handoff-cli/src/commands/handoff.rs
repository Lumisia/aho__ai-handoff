//! `ai-handoff handoff` — explicitly consume the pending capsule for this
//! project (the /handoff skill's backend). Prints the daemon's hook-style JSON
//! so skills can read `hookSpecificOutput.additionalContext`; `{}` means no
//! pending capsule targets this agent.

use crate::AgentArg;
use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, VERSION},
};
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::io::Write;
use std::time::Duration;

pub fn run(agent: AgentArg) -> anyhow::Result<i32> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    Ok(run_io(agent.as_str(), &mut out, true))
}

pub fn run_io(agent: &str, out: &mut dyn Write, autostart_daemon: bool) -> i32 {
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();

    let req = Request {
        version: VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: "handoff_consume".to_string(),
        agent: agent.to_string(),
        event: "handoff".to_string(),
        received_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        cwd: cwd.clone(),
        session_id: None,
        turn_id: None,
        raw_hook_input: json!({ "cwd": cwd }),
        client: ClientInfo {
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            platform: std::env::consts::OS.to_string(),
        },
    };

    let mut resp = send(&req, &ClientConfig::default());
    if autostart_daemon
        && super::hook::daemon_unavailable(&resp)
        && super::hook::try_start_daemon().is_ok()
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
    0
}
