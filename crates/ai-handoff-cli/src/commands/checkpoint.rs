use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, Status, VERSION},
};
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use std::io::{Read, Write};

pub fn run(message: Option<String>) -> anyhow::Result<i32> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut out = stdout.lock();
    Ok(run_io(message, &mut input, &mut out))
}

pub fn run_io(message: Option<String>, input: &mut dyn Read, out: &mut dyn Write) -> i32 {
    let mut raw_text = String::new();
    let _ = input.read_to_string(&mut raw_text);
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
        agent: "codex".to_string(),
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
