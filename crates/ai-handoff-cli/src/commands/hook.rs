use crate::AgentArg;
use ai_handoff_ipc::protocol::{ClientInfo, Request, Response, VERSION};
use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use std::io::{Read, Write};

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
    let request = build_request(event, agent, raw_text);
    let response = crate::daemon_supply::send_with_supply(&request, autostart_daemon);

    match emit_response(&response, out) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("[ai-handoff] failed to write hook response: {error}");
            1
        }
    }
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

fn emit_response(response: &Response, out: &mut dyn Write) -> std::io::Result<()> {
    for warning in &response.warnings {
        eprintln!("[ai-handoff] {warning}");
    }
    let text = serde_json::to_string(&response.hook_stdout).map_err(std::io::Error::other)?;
    writeln!(out, "{text}")?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed output"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("closed output"))
        }
    }

    #[test]
    fn response_output_failure_is_not_silently_ignored() {
        let response = ai_handoff_ipc::protocol::degraded("req", "daemon_unavailable");

        assert!(emit_response(&response, &mut FailingWriter).is_err());
    }
}
