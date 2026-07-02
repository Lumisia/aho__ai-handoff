use crate::AgentArg;
use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, Response, VERSION},
};
use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use std::io::{Read, Write};
use std::process::Stdio;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub fn run(event: &str, agent: AgentArg) -> anyhow::Result<i32> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut raw_text = String::new();
    let _ = input.read_to_string(&mut raw_text);
    Ok(run_from_raw(
        event,
        agent.as_str(),
        &raw_text,
        &mut output,
        true,
    ))
}

pub fn run_io(event: &str, agent: &str, input: &mut dyn Read, out: &mut dyn Write) -> i32 {
    let mut raw_text = String::new();
    let _ = input.read_to_string(&mut raw_text);
    run_from_raw(event, agent, &raw_text, out, false)
}

fn run_from_raw(
    event: &str,
    agent: &str,
    raw_text: &str,
    out: &mut dyn Write,
    autostart_daemon: bool,
) -> i32 {
    let req = build_request(event, agent, raw_text);
    let mut resp = send(&req, &ClientConfig::default());

    if autostart_daemon && daemon_unavailable(&resp) && try_start_daemon().is_ok() {
        resp = send(
            &req,
            &ClientConfig {
                request_timeout: Duration::from_millis(2500),
                ..ClientConfig::default()
            },
        );
    }

    emit_response(&resp, out);
    0
}

fn build_request(event: &str, agent: &str, raw_text: &str) -> Request {
    let raw = serde_json::from_str::<Value>(raw_text.trim()).unwrap_or(Value::Null);
    let cwd = raw
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default()
        });

    Request {
        version: VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: "hook_event".to_string(),
        agent: agent.to_string(),
        event: event.to_string(),
        received_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        cwd,
        session_id: raw
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        turn_id: raw
            .get("turn_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        raw_hook_input: raw,
        client: ClientInfo {
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            platform: std::env::consts::OS.to_string(),
        },
    }
}

fn emit_response(resp: &Response, out: &mut dyn Write) {
    for warning in &resp.warnings {
        eprintln!("[ai-handoff] {warning}");
    }
    let text = serde_json::to_string(&resp.hook_stdout).unwrap_or_else(|_| "{}".to_string());
    let _ = writeln!(out, "{text}");
}

pub(crate) fn daemon_unavailable(resp: &Response) -> bool {
    resp.warnings
        .iter()
        .any(|warning| warning == "daemon_unavailable")
}

pub(crate) fn try_start_daemon() -> std::io::Result<()> {
    if std::env::var("AI_HANDOFF_NO_DAEMON_AUTOSTART")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "daemon autostart disabled",
        ));
    }

    let exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(exe);
    command
        .args(["daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let _child = command.spawn()?;
    Ok(())
}
