//! `ai-handoff retarget <capsule-id> [--to <agent>]` — point a pending capsule
//! at a different agent, or open it up for anyone when --to is omitted. The
//! explicit fix-up path when a capsule was saved with the wrong target.

use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, Status, VERSION},
};
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::io::Write;
use std::time::Duration;

pub fn run(capsule_id: &str, to: Option<String>) -> anyhow::Result<i32> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    Ok(run_io(capsule_id, to, &mut out, true))
}

pub fn run_io(
    capsule_id: &str,
    to: Option<String>,
    out: &mut dyn Write,
    autostart_daemon: bool,
) -> i32 {
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut raw = json!({ "cwd": cwd, "capsule_id": capsule_id });
    // Omitting `target` opens the capsule; the daemon never guesses one.
    if let Some(target) = &to {
        raw["target"] = json!(target);
    }

    let req = Request {
        version: VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: "handoff_retarget".to_string(),
        agent: "cli".to_string(),
        event: "retarget".to_string(),
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
    if resp.status == Status::Ok {
        let text = serde_json::to_string(&resp.hook_stdout).unwrap_or_else(|_| "{}".to_string());
        let _ = writeln!(out, "{text}");
        0
    } else {
        let text = serde_json::to_string(&json!({
            "status": resp.status,
            "warnings": resp.warnings,
        }))
        .unwrap_or_else(|_| r#"{"status":"error"}"#.to_string());
        let _ = writeln!(out, "{text}");
        1
    }
}
