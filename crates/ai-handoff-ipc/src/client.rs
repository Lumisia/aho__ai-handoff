use crate::protocol::{degraded, Request, Response};
use ai_handoff_core::paths::{requests_dir, responses_dir};
use std::time::{Duration, Instant};

pub struct ClientConfig {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub poll_interval: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_millis(150),
            request_timeout: Duration::from_millis(1500),
            poll_interval: Duration::from_millis(25),
        }
    }
}

pub fn send(req: &Request, cfg: &ClientConfig) -> Response {
    cleanup_response(&req.request_id);
    if write_request(req).is_err() {
        cleanup_request(&req.request_id);
        return degraded(&req.request_id, "daemon_unavailable");
    }

    let response_path = responses_dir().join(format!("{}.json", req.request_id));
    let deadline = Instant::now() + cfg.request_timeout;

    while Instant::now() < deadline {
        match std::fs::read(&response_path) {
            Ok(bytes) => match serde_json::from_slice::<Response>(&bytes) {
                Ok(response) => {
                    cleanup_request(&req.request_id);
                    let _ = std::fs::remove_file(response_path);
                    return response;
                }
                Err(_) => {
                    cleanup_request(&req.request_id);
                    cleanup_response(&req.request_id);
                    return degraded(&req.request_id, "daemon_unavailable");
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::thread::sleep(cfg.poll_interval);
            }
            Err(_) => {
                cleanup_request(&req.request_id);
                cleanup_response(&req.request_id);
                return degraded(&req.request_id, "daemon_unavailable");
            }
        }
    }

    cleanup_request(&req.request_id);
    cleanup_response(&req.request_id);
    degraded(&req.request_id, "daemon_unavailable")
}

fn cleanup_request(request_id: &str) {
    let _ = std::fs::remove_file(requests_dir().join(format!("{request_id}.json")));
}

fn cleanup_response(request_id: &str) {
    let _ = std::fs::remove_file(responses_dir().join(format!("{request_id}.json")));
}

fn write_request(req: &Request) -> std::io::Result<()> {
    let dir = requests_dir();
    // No explicit ensure calls here: `write_private_atomic` creates + hardens
    // the parent dir itself, and the daemon hardens the full IPC tree at
    // startup. Hooks fire on every tool call, and each Windows hardening
    // spawns icacls.exe — four redundant spawns per hook add real latency.

    let path = dir.join(format!("{}.json", req.request_id));
    let tmp = dir.join(format!("{}.json.tmp", req.request_id));
    let bytes = serde_json::to_vec(req)?;
    ai_handoff_core::secure_fs::write_private_atomic(&path, &tmp, &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ClientInfo, Request, Response, Status, VERSION};
    use crate::test_support::env_lock;
    use ai_handoff_core::paths::{requests_dir, responses_dir};
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn sample_request(id: &str) -> Request {
        Request {
            version: VERSION,
            request_id: id.into(),
            kind: "hook_event".into(),
            agent: "codex".into(),
            event: "stop".into(),
            received_at: "2026-06-25T12:34:56Z".into(),
            cwd: "/repo".into(),
            session_id: Some("s1".into()),
            turn_id: None,
            raw_hook_input: json!({ "cwd": "/repo" }),
            client: ClientInfo {
                binary_version: "2.0.0-mvp".into(),
                pid: 1,
                platform: "windows".into(),
            },
        }
    }

    #[test]
    fn send_roundtrips_response_file() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::create_dir_all(requests_dir()).unwrap();
        std::fs::create_dir_all(responses_dir()).unwrap();

        let req = sample_request("req-online");
        let response_path = responses_dir().join("req-online.json");
        let request_path = requests_dir().join("req-online.json");
        let responder = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !request_path.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            let resp = Response {
                version: VERSION,
                request_id: "req-online".into(),
                status: Status::Ok,
                hook_stdout: json!({ "ok": true }),
                warnings: vec![],
                diagnostics: json!({}),
            };
            std::fs::write(response_path, serde_json::to_vec(&resp).unwrap()).unwrap();
        });

        let resp = send(
            &req,
            &ClientConfig {
                request_timeout: Duration::from_secs(2),
                poll_interval: Duration::from_millis(5),
                ..Default::default()
            },
        );
        responder.join().unwrap();
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(resp.hook_stdout["ok"], true);
        assert!(!requests_dir().join("req-online.json").exists());
        assert!(!responses_dir().join("req-online.json").exists());
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn send_returns_degraded_when_no_daemon() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::create_dir_all(requests_dir()).unwrap();
        std::fs::create_dir_all(responses_dir()).unwrap();

        let req = sample_request("req-offline");
        let started = Instant::now();
        let resp = send(
            &req,
            &ClientConfig {
                request_timeout: Duration::from_millis(80),
                poll_interval: Duration::from_millis(5),
                ..Default::default()
            },
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(resp.status, Status::Degraded);
        assert_eq!(resp.warnings, vec!["daemon_unavailable"]);
        assert!(
            !requests_dir().join("req-offline.json").exists(),
            "timed-out requests must not be processed later when the daemon starts"
        );
        std::env::remove_var("AI_HANDOFF_HOME");
    }
}
