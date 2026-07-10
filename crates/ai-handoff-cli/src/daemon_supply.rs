use std::process::Stdio;
use std::time::{Duration, Instant};

use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, Response, Status, VERSION},
};
use anyhow::Context;
use chrono::{SecondsFormat, Utc};
use serde::Serialize;

const SUPPLY_DEADLINE: Duration = Duration::from_secs(4);
const HOST_LAUNCH_WINDOW: Duration = Duration::from_secs(2);
const HEALTH_POLL: Duration = Duration::from_millis(50);
const HEALTH_REQUEST_TIMEOUT: Duration = Duration::from_millis(120);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupplyStrategy {
    AlreadyRunning,
    HostLauncher,
    Direct,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupplyOutcome {
    pub ready: bool,
    pub strategy: SupplyStrategy,
    pub errors: Vec<String>,
}

pub fn ensure_daemon() -> anyhow::Result<SupplyOutcome> {
    if daemon_autostart_disabled() {
        return Ok(SupplyOutcome {
            ready: false,
            strategy: SupplyStrategy::Unavailable,
            errors: vec!["daemon autostart is disabled by AI_HANDOFF_NO_DAEMON_AUTOSTART".into()],
        });
    }

    let state = ai_handoff_core::install::state::load(&ai_handoff_core::paths::home());
    let host_state = state.host_launcher.as_ref();
    let started = Instant::now();
    let deadline = started + SUPPLY_DEADLINE;
    let host_deadline = started + HOST_LAUNCH_WINDOW;

    Ok(ensure_with(
        host_state.is_some(),
        || ping_daemon(HEALTH_REQUEST_TIMEOUT),
        || crate::host_launcher::launch(host_state),
        try_start_direct,
        |strategy| {
            let stage_deadline = match strategy {
                SupplyStrategy::HostLauncher => host_deadline,
                SupplyStrategy::Direct => deadline,
                _ => Instant::now(),
            };
            wait_until_healthy(stage_deadline)
        },
    ))
}

pub fn send_with_supply(req: &Request, autostart_daemon: bool) -> Response {
    let mut attempt = 0_u8;
    send_with_supply_with(
        req,
        autostart_daemon,
        |request| {
            attempt += 1;
            let request_timeout = if attempt == 1 {
                ClientConfig::default().request_timeout
            } else {
                Duration::from_millis(2500)
            };
            send(
                request,
                &ClientConfig {
                    request_timeout,
                    ..ClientConfig::default()
                },
            )
        },
        || {
            ensure_daemon().unwrap_or_else(|error| SupplyOutcome {
                ready: false,
                strategy: SupplyStrategy::Unavailable,
                errors: vec![error.to_string()],
            })
        },
    )
}

pub(crate) fn ping_daemon(timeout: Duration) -> bool {
    response_is_healthy(&ping_response(timeout))
}

pub(crate) fn response_is_healthy(response: &Response) -> bool {
    response.status == Status::Ok
        && response
            .hook_stdout
            .get("pong")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && response
            .hook_stdout
            .get("store_writable")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
}

fn ping_response(timeout: Duration) -> Response {
    let request = Request {
        version: VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: "ping".to_string(),
        agent: "cli".to_string(),
        event: "ping".to_string(),
        received_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        cwd: std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        session_id: None,
        turn_id: None,
        raw_hook_input: serde_json::json!({}),
        client: ClientInfo {
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            platform: std::env::consts::OS.to_string(),
        },
    };
    send(
        &request,
        &ClientConfig {
            request_timeout: timeout,
            poll_interval: Duration::from_millis(10),
            ..ClientConfig::default()
        },
    )
}

fn ensure_with<Ping, Host, Direct, Wait>(
    has_host_launcher: bool,
    mut ping: Ping,
    mut launch_host: Host,
    mut launch_direct: Direct,
    mut wait_healthy: Wait,
) -> SupplyOutcome
where
    Ping: FnMut() -> bool,
    Host: FnMut() -> anyhow::Result<()>,
    Direct: FnMut() -> anyhow::Result<()>,
    Wait: FnMut(SupplyStrategy) -> bool,
{
    if ping() {
        return SupplyOutcome {
            ready: true,
            strategy: SupplyStrategy::AlreadyRunning,
            errors: Vec::new(),
        };
    }

    let mut errors = Vec::new();
    if has_host_launcher {
        match launch_host() {
            Ok(()) if wait_healthy(SupplyStrategy::HostLauncher) => {
                return SupplyOutcome {
                    ready: true,
                    strategy: SupplyStrategy::HostLauncher,
                    errors,
                };
            }
            Ok(()) => errors.push("host launcher started, but no healthy daemon answered".into()),
            Err(error) => errors.push(format!("host launcher: {error}")),
        }
    }

    match launch_direct() {
        Ok(()) if wait_healthy(SupplyStrategy::Direct) => SupplyOutcome {
            ready: true,
            strategy: SupplyStrategy::Direct,
            errors,
        },
        Ok(()) => {
            errors.push("direct daemon started, but no healthy daemon answered".into());
            SupplyOutcome {
                ready: false,
                strategy: SupplyStrategy::Unavailable,
                errors,
            }
        }
        Err(error) => {
            errors.push(format!("direct daemon: {error}"));
            SupplyOutcome {
                ready: false,
                strategy: SupplyStrategy::Unavailable,
                errors,
            }
        }
    }
}

fn send_with_supply_with<Send, Ensure>(
    req: &Request,
    autostart_daemon: bool,
    mut send_request: Send,
    mut ensure: Ensure,
) -> Response
where
    Send: FnMut(&Request) -> Response,
    Ensure: FnMut() -> SupplyOutcome,
{
    let response = send_request(req);
    if !autostart_daemon || !daemon_unavailable(&response) {
        return response;
    }

    let outcome = ensure();
    if outcome.ready {
        return send_request(req);
    }
    supply_failure_response(req, outcome)
}

fn daemon_unavailable(response: &Response) -> bool {
    response
        .warnings
        .iter()
        .any(|warning| warning == "daemon_unavailable")
}

fn supply_failure_response(req: &Request, outcome: SupplyOutcome) -> Response {
    Response {
        version: VERSION,
        request_id: req.request_id.clone(),
        status: Status::Degraded,
        hook_stdout: serde_json::json!({
            "systemMessage": "AI Handoff could not start a host-capable daemon. Automatic checkpointing is temporarily unavailable; run `ai-handoff doctor --fix`.",
        }),
        warnings: vec!["daemon_unavailable".into(), "daemon_supply_failed".into()],
        diagnostics: serde_json::json!({
            "supply_strategy": outcome.strategy,
            "errors": outcome.errors,
        }),
    }
}

fn wait_until_healthy(deadline: Instant) -> bool {
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if ping_daemon(remaining.min(HEALTH_REQUEST_TIMEOUT)) {
            return true;
        }
        std::thread::sleep(remaining.min(HEALTH_POLL));
    }
    false
}

fn daemon_autostart_disabled() -> bool {
    std::env::var("AI_HANDOFF_NO_DAEMON_AUTOSTART")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn try_start_direct() -> anyhow::Result<()> {
    let cli = std::env::current_exe().context("resolve current ai-handoff executable")?;
    let host = crate::host_launcher::resolve_host_executable(&cli)?;
    spawn_daemon_detached(&host, &ai_handoff_core::paths::home())
        .context("spawn native background host directly")
}

fn direct_daemon_command(host: &std::path::Path, home: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new(host);
    command
        .arg("--home")
        .arg(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn spawn_daemon_detached(host: &std::path::Path, home: &std::path::Path) -> std::io::Result<()> {
    direct_daemon_command(host, home).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_ipc::protocol::{degraded, Response};
    use std::cell::{Cell, RefCell};

    #[test]
    fn direct_command_targets_native_host_without_cli_subcommands() {
        let host = std::path::Path::new(if cfg!(windows) {
            r"C:\home\bin\ai-handoff-host.exe"
        } else {
            "/home/me/.ai-handoff/bin/ai-handoff-host"
        });
        let home = std::path::Path::new(if cfg!(windows) {
            r"C:\home"
        } else {
            "/home/me/.ai-handoff"
        });
        let command = direct_daemon_command(host, home);
        let args: Vec<_> = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();

        assert_eq!(command.get_program(), host.as_os_str());
        assert_eq!(args, vec!["--home", &home.to_string_lossy()]);
        assert!(!args.iter().any(|arg| arg == "daemon" || arg == "run"));
    }

    #[test]
    fn healthy_ping_skips_both_launch_strategies() {
        let events = RefCell::new(Vec::new());

        let outcome = ensure_with(
            true,
            || true,
            || {
                events.borrow_mut().push("host");
                Ok(())
            },
            || {
                events.borrow_mut().push("direct");
                Ok(())
            },
            |_| false,
        );

        assert_eq!(outcome.strategy, SupplyStrategy::AlreadyRunning);
        assert!(outcome.ready);
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn recorded_host_launcher_runs_before_direct_fallback() {
        let events = RefCell::new(Vec::new());

        let outcome = ensure_with(
            true,
            || false,
            || {
                events.borrow_mut().push("host");
                Ok(())
            },
            || {
                events.borrow_mut().push("direct");
                Ok(())
            },
            |strategy| {
                events.borrow_mut().push(match strategy {
                    SupplyStrategy::HostLauncher => "host-ready",
                    SupplyStrategy::Direct => "direct-ready",
                    _ => "unexpected",
                });
                strategy == SupplyStrategy::HostLauncher
            },
        );

        assert_eq!(outcome.strategy, SupplyStrategy::HostLauncher);
        assert!(outcome.ready);
        assert_eq!(&*events.borrow(), &["host", "host-ready"]);
    }

    #[test]
    fn failed_host_launcher_falls_back_to_direct_supply() {
        let events = RefCell::new(Vec::new());

        let outcome = ensure_with(
            true,
            || false,
            || {
                events.borrow_mut().push("host");
                anyhow::bail!("host denied")
            },
            || {
                events.borrow_mut().push("direct");
                Ok(())
            },
            |strategy| {
                events.borrow_mut().push("direct-ready");
                strategy == SupplyStrategy::Direct
            },
        );

        assert_eq!(outcome.strategy, SupplyStrategy::Direct);
        assert!(outcome.ready);
        assert_eq!(&*events.borrow(), &["host", "direct", "direct-ready"]);
        assert!(outcome
            .errors
            .iter()
            .any(|error| error.contains("host denied")));
    }

    #[test]
    fn failed_host_and_direct_supply_report_unavailable() {
        let outcome = ensure_with(
            true,
            || false,
            || anyhow::bail!("host denied"),
            || anyhow::bail!("direct denied"),
            |_| false,
        );

        assert_eq!(outcome.strategy, SupplyStrategy::Unavailable);
        assert!(!outcome.ready);
        assert_eq!(outcome.errors.len(), 2);
    }

    #[test]
    fn original_request_is_retried_exactly_once_after_readiness() {
        let sends = Cell::new(0);
        let supply_completed = Cell::new(false);
        let req = sample_request();

        let response = send_with_supply_with(
            &req,
            true,
            |_| {
                sends.set(sends.get() + 1);
                if sends.get() == 1 {
                    degraded("req-supply", "daemon_unavailable")
                } else {
                    assert!(supply_completed.get());
                    healthy_response()
                }
            },
            || {
                supply_completed.set(true);
                SupplyOutcome {
                    ready: true,
                    strategy: SupplyStrategy::HostLauncher,
                    errors: Vec::new(),
                }
            },
        );

        assert_eq!(sends.get(), 2);
        assert_eq!(response.hook_stdout["ok"], true);
    }

    fn sample_request() -> ai_handoff_ipc::protocol::Request {
        ai_handoff_ipc::protocol::Request {
            version: ai_handoff_ipc::protocol::VERSION,
            request_id: "req-supply".into(),
            kind: "hook_event".into(),
            agent: "codex".into(),
            event: "stop".into(),
            received_at: "2026-07-10T00:00:00Z".into(),
            cwd: "C:\\repo".into(),
            session_id: Some("s1".into()),
            turn_id: None,
            raw_hook_input: serde_json::json!({}),
            client: ai_handoff_ipc::protocol::ClientInfo {
                binary_version: "test".into(),
                pid: 1,
                platform: "windows".into(),
            },
        }
    }

    fn healthy_response() -> Response {
        Response {
            version: ai_handoff_ipc::protocol::VERSION,
            request_id: "req-supply".into(),
            status: ai_handoff_ipc::protocol::Status::Ok,
            hook_stdout: serde_json::json!({ "ok": true }),
            warnings: Vec::new(),
            diagnostics: serde_json::json!({}),
        }
    }
}
