use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, Status, VERSION},
};
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::io::Write;
use std::time::Duration;

pub fn run(json_output: bool) -> anyhow::Result<i32> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    Ok(run_io(json_output, &mut out))
}

pub fn run_io(json_output: bool, out: &mut dyn Write) -> i32 {
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

    // Per-agent plugin install state, read from the recorded install-state.
    let st = ai_handoff_core::install::state::load(&ai_handoff_core::paths::home());
    let plugin_state = |rec: &Option<ai_handoff_core::install::PluginRecord>| match rec {
        Some(r) => json!({ "installed": true, "root": r.root }),
        None => json!({ "installed": false }),
    };
    let claude_plugin = plugin_state(&st.claude.plugin);
    let codex_plugin = plugin_state(&st.codex.plugin);

    let report = json!({
        "daemon": daemon,
        "home": ai_handoff_core::paths::home().to_string_lossy(),
        "ipc": ai_handoff_core::paths::ipc_dir().to_string_lossy(),
        "plugin": {
            "claude": claude_plugin,
            "codex": codex_plugin,
        },
    });

    if json_output {
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        let _ = writeln!(out, "daemon: {daemon}");
        let mark = |installed: bool| if installed { "installed" } else { "not installed" };
        let _ = writeln!(
            out,
            "claude plugin: {}",
            mark(st.claude.plugin.is_some())
        );
        let _ = writeln!(out, "codex plugin: {}", mark(st.codex.plugin.is_some()));
    }
    0
}
