use crate::AgentArg;
use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, Response, Status, VERSION},
};
use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use std::io::{Read, Write};
use std::process::Stdio;
use std::time::{Duration, Instant};

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

    if autostart_daemon && daemon_unavailable(&resp) && start_daemon_logged() {
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

pub(crate) fn ping_daemon(timeout: Duration) -> bool {
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let req = Request {
        version: VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: "ping".to_string(),
        agent: "cli".to_string(),
        event: "ping".to_string(),
        received_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        cwd,
        session_id: None,
        turn_id: None,
        raw_hook_input: serde_json::json!({}),
        client: ClientInfo {
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            platform: std::env::consts::OS.to_string(),
        },
    };
    let resp = send(
        &req,
        &ClientConfig {
            request_timeout: timeout,
            poll_interval: Duration::from_millis(10),
            ..ClientConfig::default()
        },
    );
    resp.status == Status::Ok
}

/// Autostart the daemon and say WHY when it fails. The silent version left
/// users staring at "process is running but hooks do nothing" with no clue.
pub(crate) fn start_daemon_logged() -> bool {
    match try_start_daemon() {
        Ok(()) => true,
        Err(error) => {
            eprintln!("[ai-handoff] daemon autostart failed: {error}");
            false
        }
    }
}

pub(crate) fn try_start_daemon() -> std::io::Result<()> {
    if std::env::var("AI_HANDOFF_NO_DAEMON_AUTOSTART")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return Err(std::io::Error::other("daemon autostart disabled"));
    }

    let exe = std::env::current_exe()?;
    spawn_daemon_detached(&exe)?;
    let deadline = Instant::now() + Duration::from_millis(2500);
    while Instant::now() < deadline {
        if ping_daemon(Duration::from_millis(100)) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "daemon did not become reachable",
    ))
}

/// Start `<exe> daemon run` without inheriting THIS process's handle table.
///
/// A plain std spawn on Windows passes `bInheritHandles=TRUE`, so the stdout
/// pipe the calling shell (or an agent's hook runner) attached to this hook
/// leaks into the long-lived daemon. The caller then never sees EOF until the
/// daemon's idle exit — a cold-start hook measured 62s against a 10s hook
/// timeout. `Start-Process` goes through ShellExecuteEx, which starts the
/// daemon with a fresh handle table, so the hook's pipes close when the hook
/// exits.
#[cfg(windows)]
fn spawn_daemon_detached(exe: &std::path::Path) -> std::io::Result<()> {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = format!(
        "Start-Process -FilePath '{}' -ArgumentList 'daemon','run' -WindowStyle Hidden",
        exe.to_string_lossy().replace('\'', "''"),
    );
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "Start-Process for the daemon exited with {status}"
        )))
    }
}

#[cfg(not(windows))]
fn spawn_daemon_detached(exe: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new(exe)
        .args(["daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}
