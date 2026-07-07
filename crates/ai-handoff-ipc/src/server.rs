use crate::protocol::{Request, Response};
use ai_handoff_core::paths::{dead_letter_dir, requests_dir, responses_dir};
use std::path::Path;
use std::time::{Duration, Instant};

pub trait Handler {
    fn handle(&self, req: &Request) -> Response;
}

pub fn serve_once(handler: &dyn Handler) -> usize {
    let Ok(entries) = std::fs::read_dir(requests_dir()) else {
        // Request dir missing (first run, or wiped mid-run): (re)create the IPC
        // dirs once and report no work. The next poll serves normally. Keeping
        // the ensure/harden calls OUT of the success path matters: on Windows
        // each hardening spawns icacls.exe, and doing that per poll turned the
        // idle daemon into a constant process-spawn loop.
        ensure_ipc_dirs();
        return 0;
    };

    let mut processed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_request_file(&path) {
            continue;
        }

        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let req = match serde_json::from_slice::<Request>(&bytes) {
            Ok(req) => req,
            Err(_) => {
                move_to_dead_letter(&path);
                continue;
            }
        };
        // The response path is `responses/<request_id>.json`, and the id comes
        // straight from the request body. Reject anything that could escape
        // the responses dir (e.g. `../../…`) BEFORE handling: on Windows the
        // request dir is deliberately writable by sandboxed agent hooks, so an
        // unvalidated id would let a sandboxed process use the unsandboxed
        // daemon to write files at arbitrary paths.
        if !valid_request_id(&req.request_id) {
            move_to_dead_letter(&path);
            continue;
        }

        let response = handler.handle(&req);
        if write_response(&response).is_ok() {
            let _ = std::fs::remove_file(&path);
            processed += 1;
        }
    }

    processed
}

/// The idle-poll ceiling for [`serve_forever`]'s adaptive backoff. Clients wait
/// up to 1.5s for a response by default, so a 400ms worst-case pickup delay
/// stays well inside that budget while keeping the idle daemon near-silent.
const MAX_IDLE_POLL: Duration = Duration::from_millis(400);

pub fn serve_forever(handler: &dyn Handler, poll: Duration) -> ! {
    ensure_ipc_dirs();
    let max_poll = MAX_IDLE_POLL.max(poll);
    let mut current = poll;
    loop {
        if serve_once(handler) > 0 {
            // Active burst: snap back to the fast poll for low hook latency.
            current = poll;
        } else {
            // Idle: back off exponentially so a quiet daemon costs almost
            // nothing (no dir scans 40×/sec in the background).
            current = (current * 2).min(max_poll);
        }
        std::thread::sleep(current);
    }
}

pub fn serve_until_idle(handler: &dyn Handler, poll: Duration, idle_timeout: Duration) -> usize {
    ensure_ipc_dirs();
    let max_poll = MAX_IDLE_POLL.max(poll);
    let mut current = poll;
    let mut idle_since = Instant::now();
    let mut processed_total = 0;

    loop {
        let processed = serve_once(handler);
        if processed > 0 {
            processed_total += processed;
            current = poll;
            idle_since = Instant::now();
        } else if idle_since.elapsed() >= idle_timeout {
            return processed_total;
        } else {
            current = (current * 2).min(max_poll);
        }
        std::thread::sleep(current.min(idle_timeout.saturating_sub(idle_since.elapsed())));
    }
}

/// Create the IPC tree. Called once at daemon startup and again only if the
/// request dir disappears — never in the hot poll loop.
///
/// Only the HOME and IPC ROOT are hardened private. The subdirectories must
/// INHERIT the root ACL: on Windows the Codex sandbox grants its restricted
/// token an ACE on the IPC root (a configured writable root), and hardening
/// the subdirs with `/inheritance:r` stripped that ACE — sandboxed hooks then
/// could not write requests or read responses, and every hook degraded to
/// `daemon_unavailable` even with the daemon running. ensure_inherited_subdir
/// also repairs dirs broken by older versions.
fn ensure_ipc_dirs() {
    let _ = ai_handoff_core::secure_fs::ensure_private_dir(&ai_handoff_core::paths::home());
    let _ = ai_handoff_core::secure_fs::ensure_private_dir(&ai_handoff_core::paths::ipc_dir());
    let _ = ai_handoff_core::secure_fs::ensure_inherited_subdir(&requests_dir());
    let _ = ai_handoff_core::secure_fs::ensure_inherited_subdir(&responses_dir());
    let _ = ai_handoff_core::secure_fs::ensure_inherited_subdir(&dead_letter_dir());
}

/// A request id must be a plain token (UUIDs and the test ids qualify): it is
/// used verbatim as a file name under `responses/`.
fn valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn is_request_file(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("json")
}

fn write_response(resp: &Response) -> std::io::Result<()> {
    let dir = responses_dir();
    let path = dir.join(format!("{}.json", resp.request_id));
    let tmp = dir.join(format!("{}.json.tmp", resp.request_id));
    let bytes = serde_json::to_vec(resp)?;
    // Shared (inheriting) write: a response hardened with an explicit private
    // ACL cannot be READ by a sandboxed hook, so the client would time out
    // and report daemon_unavailable even though we answered.
    ai_handoff_core::secure_fs::write_shared_atomic(&path, &tmp, &bytes)?;
    Ok(())
}

fn move_to_dead_letter(path: &Path) {
    let _ = ai_handoff_core::secure_fs::ensure_inherited_subdir(&dead_letter_dir());
    if let Some(name) = path.file_name() {
        let dest = dead_letter_dir().join(name);
        let _ = std::fs::remove_file(&dest);
        if std::fs::rename(path, &dest).is_err() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ClientInfo, Request, Response, Status, VERSION};
    use crate::test_support::env_lock;
    use ai_handoff_core::paths::{dead_letter_dir, requests_dir, responses_dir};
    use serde_json::json;

    struct EchoHandler;
    impl Handler for EchoHandler {
        fn handle(&self, req: &Request) -> Response {
            Response {
                version: VERSION,
                request_id: req.request_id.clone(),
                status: Status::Ok,
                hook_stdout: json!({ "ok": true }),
                warnings: vec![],
                diagnostics: json!({}),
            }
        }
    }

    fn sample_request(id: &str) -> Request {
        Request {
            version: VERSION,
            request_id: id.into(),
            kind: "hook_event".into(),
            agent: "codex".into(),
            event: "stop".into(),
            received_at: "2026-06-25T12:34:56Z".into(),
            cwd: "/repo".into(),
            session_id: None,
            turn_id: None,
            raw_hook_input: json!({}),
            client: ClientInfo {
                binary_version: "2.0.0-mvp".into(),
                pid: 1,
                platform: "windows".into(),
            },
        }
    }

    #[test]
    fn serve_once_writes_response_and_deletes_request() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::create_dir_all(requests_dir()).unwrap();
        std::fs::create_dir_all(responses_dir()).unwrap();
        let req = sample_request("req-server");
        let req_path = requests_dir().join("req-server.json");
        std::fs::write(&req_path, serde_json::to_vec(&req).unwrap()).unwrap();

        assert_eq!(serve_once(&EchoHandler), 1);
        assert!(!req_path.exists());
        let resp: Response = serde_json::from_slice(
            &std::fs::read(responses_dir().join("req-server.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(resp.request_id, "req-server");
        assert_eq!(resp.hook_stdout["ok"], true);
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn serve_once_dead_letters_path_traversal_request_ids() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::create_dir_all(requests_dir()).unwrap();
        std::fs::create_dir_all(responses_dir()).unwrap();
        // A well-formed request whose id tries to escape the responses dir.
        let req = sample_request("../../escape");
        let req_path = requests_dir().join("evil.json");
        std::fs::write(&req_path, serde_json::to_vec(&req).unwrap()).unwrap();

        assert_eq!(serve_once(&EchoHandler), 0);
        assert!(!req_path.exists(), "request must be consumed");
        assert!(
            dead_letter_dir().join("evil.json").exists(),
            "invalid ids go to dead-letter"
        );
        // Nothing may be written outside responses/ — the traversal target
        // would land at <home>/escape.json.
        assert!(!home.path().join("escape.json").exists());
        assert_eq!(
            std::fs::read_dir(responses_dir()).unwrap().count(),
            0,
            "no response file for a rejected id"
        );
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn valid_request_id_accepts_uuid_and_rejects_separators() {
        assert!(valid_request_id("0197e5c3-1111-2222-3333-444455556666"));
        assert!(valid_request_id("req-server_1"));
        assert!(!valid_request_id(""));
        assert!(!valid_request_id("../../escape"));
        assert!(!valid_request_id("a/b"));
        assert!(!valid_request_id("a\\b"));
        assert!(!valid_request_id("a.b"));
        assert!(!valid_request_id(&"x".repeat(65)));
    }

    #[test]
    fn serve_once_moves_malformed_requests_to_dead_letter() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::create_dir_all(requests_dir()).unwrap();
        let req_path = requests_dir().join("bad.json");
        std::fs::write(&req_path, b"not json").unwrap();

        assert_eq!(serve_once(&EchoHandler), 0);
        assert!(!req_path.exists());
        assert!(dead_letter_dir().join("bad.json").exists());
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn serve_until_idle_returns_after_timeout_without_requests() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let started = std::time::Instant::now();
        let processed = serve_until_idle(
            &EchoHandler,
            Duration::from_millis(1),
            Duration::from_millis(20),
        );

        assert_eq!(processed, 0);
        assert!(started.elapsed() >= Duration::from_millis(20));
        assert!(started.elapsed() < Duration::from_secs(1));
        std::env::remove_var("AI_HANDOFF_HOME");
    }
}
