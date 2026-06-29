use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, Status, VERSION},
};
use anyhow::Context;
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use std::io::{Read, Write};

pub fn run(
    message: Option<String>,
    agent: Option<String>,
    file: Option<std::path::PathBuf>,
) -> anyhow::Result<i32> {
    // --file bypasses stdin, which several shells (notably PowerShell) do not
    // pipe to native executables reliably; fall back to stdin when absent.
    let mut raw_text = String::new();
    if let Some(path) = file {
        raw_text = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read capsule file {}", path.display()))?;
    } else {
        let stdin = std::io::stdin();
        let _ = stdin.lock().read_to_string(&mut raw_text);
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    Ok(run_io(message, agent, &raw_text, &mut out))
}

pub fn run_io(
    message: Option<String>,
    agent: Option<String>,
    raw_text: &str,
    out: &mut dyn Write,
) -> i32 {
    let input_json = serde_json::from_str::<Value>(raw_text.trim()).unwrap_or(Value::Null);
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let message = message
        .or_else(|| {
            input_json
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "manual checkpoint".to_string());
    // Source agent sets the handoff direction. Precedence: --agent flag, then a
    // stdin `agent` field, then default codex. Normalize aliases to the values
    // the daemon's parse_agent accepts.
    let agent = agent
        .or_else(|| {
            input_json
                .get("agent")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .map(|value| normalize_agent(&value))
        .unwrap_or_else(|| "codex".to_string());

    let mut raw_hook_input = if input_json.is_object() {
        input_json
    } else {
        json!({})
    };
    if let Some(obj) = raw_hook_input.as_object_mut() {
        obj.insert("cwd".to_string(), json!(cwd.clone()));
        obj.entry("message".to_string())
            .or_insert_with(|| json!(message.clone()));
    }

    let req = Request {
        version: VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: "checkpoint".to_string(),
        agent: agent.clone(),
        event: "checkpoint".to_string(),
        received_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        cwd: cwd.clone(),
        session_id: None,
        turn_id: None,
        raw_hook_input,
        client: ClientInfo {
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            platform: std::env::consts::OS.to_string(),
        },
    };

    let resp = send(&req, &ClientConfig::default());
    let text = serde_json::to_string(&resp.hook_stdout).unwrap_or_else(|_| "{}".to_string());
    let _ = writeln!(out, "{text}");
    if resp.status == Status::Ok {
        0
    } else {
        1
    }
}

fn normalize_agent(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claude_code" | "claudecode" => "claude-code".to_string(),
        "codex" => "codex".to_string(),
        other => other.to_string(),
    }
}
