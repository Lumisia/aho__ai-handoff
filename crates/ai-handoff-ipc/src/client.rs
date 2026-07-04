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
                    std::thread::sleep(cfg.poll_interval);
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
    // Shared (inheriting) write, and no per-call dir hardening. The old
    // `write_private_atomic` re-hardened the requests dir on EVERY call,
    // which both spawned icacls.exe per hook (latency) and re-broke the
    // inherited sandbox ACE that Codex needs — an unsandboxed Claude hook
    // would silently lock the sandboxed Codex hooks out again.
    let path = dir.join(format!("{}.json", req.request_id));
    let tmp = dir.join(format!("{}.json.tmp", req.request_id));
    let bytes = serde_json::to_vec(req)?;
    ai_handoff_core::secure_fs::write_shared_atomic(&path, &tmp, &bytes)?;
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
        let response_tmp = responses_dir().join("req-online.json.tmp");
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
            let bytes = serde_json::to_vec(&resp).unwrap();
            ai_handoff_core::secure_fs::write_shared_atomic(&response_path, &response_tmp, &bytes)
                .unwrap();
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
    fn send_waits_for_response_file_to_finish_writing() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::create_dir_all(requests_dir()).unwrap();
        std::fs::create_dir_all(responses_dir()).unwrap();

        let req = sample_request("req-partial");
        let response_path = responses_dir().join("req-partial.json");
        let response_tmp = responses_dir().join("req-partial.json.tmp");
        let request_path = requests_dir().join("req-partial.json");
        let responder = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !request_path.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::fs::write(&response_path, br#"{"status":"#).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            let resp = Response {
                version: VERSION,
                request_id: "req-partial".into(),
                status: Status::Ok,
                hook_stdout: json!({ "ok": true }),
                warnings: vec![],
                diagnostics: json!({}),
            };
            let bytes = serde_json::to_vec(&resp).unwrap();
            ai_handoff_core::secure_fs::write_shared_atomic(&response_path, &response_tmp, &bytes)
                .unwrap();
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
