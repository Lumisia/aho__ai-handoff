use ai_handoff_core::{
    config, fingerprint::fingerprint, install::state::load, paths,
    sensor::record_claude_rate_limit, statusline::segment,
};
use ai_handoff_daemon::store::find_pending;
use chrono::Utc;
use serde_json::Value;
use std::io::{Read, Write};
use std::path::Path;

pub fn run() -> anyhow::Result<i32> {
    let cfg = config::load();
    let now_ms = Utc::now().timestamp_millis();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    Ok(run_io(&mut input, &mut output, now_ms, cfg.statusline.show))
}

pub fn run_io(input: &mut dyn Read, out: &mut dyn Write, now_ms: i64, show: bool) -> i32 {
    // Read all stdin; treat empty or unparseable as Value::Null.
    let mut raw_text = String::new();
    let _ = input.read_to_string(&mut raw_text);
    let json: Value = serde_json::from_str(raw_text.trim()).unwrap_or(Value::Null);

    // Record the rate-limit sample (ignore result — never error).
    record_claude_rate_limit(&json, now_ms);

    // Extract used_percent from rate_limits.five_hour.used_percentage.
    let used_percent: Option<f64> = json
        .get("rate_limits")
        .and_then(|rl| rl.get("five_hour"))
        .and_then(|fh| fh.get("used_percentage"))
        .and_then(Value::as_f64);

    // Derive cwd from input.cwd or input.workspace.current_dir.
    let cwd_str: Option<String> = json
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            json.get("workspace")
                .and_then(|ws| ws.get("current_dir"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });

    // Check pending capsule when cwd is known.
    let pending = cwd_str
        .as_deref()
        .map(|cwd| {
            let path = std::path::Path::new(cwd);
            let project_id = fingerprint(path);
            // The statusline runs inside Claude Code — show capsules Claude
            // could claim (addressed to it or open).
            find_pending(&project_id, "claude-code").is_some()
        })
        .unwrap_or(false);

    // Render our segment.
    let seg = segment(used_percent, pending, show);

    // Fetch previous statusline output (best-effort; None on any failure).
    let prev =
        previous_command_from_state(&paths::home()).and_then(|cmd| run_previous(&cmd, &raw_text));

    // Combine our segment with the previous statusline output.
    let final_out = combine(&seg, prev.as_deref());

    if !final_out.is_empty() {
        let _ = write!(out, "{final_out}");
    }

    0
}

/// Pure concat logic (v1 parity).
///
/// - seg non-empty AND prev present & non-empty → `"{seg} | {prev_trimmed}"`
/// - seg non-empty AND no/empty prev → `seg` (unchanged)
/// - seg empty → the prev string (or empty when none)
///
/// Trailing whitespace/newlines are trimmed from `prev` before combining.
fn combine(seg: &str, prev: Option<&str>) -> String {
    let prev_trimmed = prev.map(|p| p.trim_end()).unwrap_or("");
    match (seg.is_empty(), prev_trimmed.is_empty()) {
        (false, false) => format!("{seg} | {prev_trimmed}"),
        (false, true) => seg.to_string(),
        (true, _) => prev_trimmed.to_string(),
    }
}

/// Load install-state and extract `.claude.statusline.previous.command`.
/// Returns None if:
/// - no state file exists
/// - no statusline state recorded
/// - no previous value
/// - no "command" key in previous object
/// - the command contains "statusline" (self-reference guard)
fn previous_command_from_state(home: &Path) -> Option<String> {
    let state = load(home);
    let cmd = state
        .claude
        .statusline
        .as_ref()?
        .previous
        .as_ref()?
        .get("command")?
        .as_str()?
        .to_string();

    // Self-reference guard: skip if the command contains "statusline"
    if cmd.contains("statusline") {
        return None;
    }

    Some(cmd)
}

/// Spawn the command through the OS shell, feeding `raw_input` on stdin.
/// Returns captured stdout on success (Some), or None on any spawn error /
/// non-zero exit (best-effort, NEVER panics, failures go to stderr only).
fn run_previous(command: &str, raw_input: &str) -> Option<String> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    #[cfg(windows)]
    let mut child = {
        Command::new("cmd")
            .args(["/C", command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| eprintln!("ai-handoff: previous statusline spawn error: {e}"))
            .ok()?
    };

    #[cfg(not(windows))]
    let mut child = {
        Command::new("sh")
            .args(["-c", command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| eprintln!("ai-handoff: previous statusline spawn error: {e}"))
            .ok()?
    };

    // Write raw_input to stdin then close the pipe.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(raw_input.as_bytes());
        // stdin is dropped/closed here
    }

    let output = child
        .wait_with_output()
        .map_err(|e| eprintln!("ai-handoff: previous statusline wait error: {e}"))
        .ok()?;

    if !output.status.success() {
        eprintln!(
            "ai-handoff: previous statusline exited with status {}",
            output.status
        );
        return None;
    }

    String::from_utf8(output.stdout)
        .map_err(|e| eprintln!("ai-handoff: previous statusline stdout not UTF-8: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_core::install::state::{save, ClaudeState, ClaudeStatuslineState, InstallState};
    use std::io::Cursor;
    use std::sync::{Mutex, MutexGuard};

    // Serialise all env-mutating tests (AI_HANDOFF_HOME) into one mutex.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    const NOW_MS: i64 = 1_750_000_000_000;

    fn run_with(json: &str, show: bool) -> (String, i32) {
        let mut input = Cursor::new(json.as_bytes().to_vec());
        let mut out: Vec<u8> = Vec::new();
        let code = run_io(&mut input, &mut out, NOW_MS, show);
        (String::from_utf8(out).unwrap(), code)
    }

    // ── Task 3 existing tests (must still pass) ─────────────────────────────

    #[test]
    fn with_used_percentage_prints_segment_and_exits_zero() {
        let _g = lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let payload = r#"{
            "session_id": "sess-abc",
            "rate_limits": { "five_hour": { "used_percentage": 42.0 } }
        }"#;
        let (out, code) = run_with(payload, true);
        assert_eq!(code, 0);
        assert_eq!(out, "AH 42%");

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn empty_stdin_prints_ah_and_exits_zero() {
        let _g = lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let (out, code) = run_with("", true);
        assert_eq!(code, 0);
        assert_eq!(out, "AH");

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn show_false_produces_empty_output_and_exits_zero() {
        let _g = lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let payload = r#"{
            "session_id": "sess-xyz",
            "rate_limits": { "five_hour": { "used_percentage": 55.0 } }
        }"#;
        let (out, code) = run_with(payload, false);
        assert_eq!(code, 0);
        assert!(
            out.is_empty(),
            "show=false must produce empty output, got: {out:?}"
        );

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn invalid_json_stdin_exits_zero_and_prints_ah() {
        let _g = lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let (out, code) = run_with("NOT JSON {{{", true);
        assert_eq!(code, 0);
        assert_eq!(out, "AH");

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn workspace_current_dir_fallback_for_cwd() {
        let _g = lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        // Use workspace.current_dir instead of cwd — still exits 0.
        let payload = r#"{
            "session_id": "sess-ws",
            "rate_limits": { "five_hour": { "used_percentage": 75.0 } },
            "workspace": { "current_dir": "C:\\some\\project" }
        }"#;
        let (out, code) = run_with(payload, true);
        assert_eq!(code, 0);
        assert_eq!(out, "AH 75%");

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    // ── Task 5: combine() unit tests ────────────────────────────────────────

    #[test]
    fn combine_seg_and_prev_concatenates_with_pipe() {
        assert_eq!(combine("AH 42%", Some("my-prompt")), "AH 42% | my-prompt");
    }

    #[test]
    fn combine_seg_only_when_no_prev() {
        assert_eq!(combine("AH 42%", None), "AH 42%");
    }

    #[test]
    fn combine_seg_only_when_prev_empty() {
        assert_eq!(combine("AH 42%", Some("")), "AH 42%");
    }

    #[test]
    fn combine_empty_seg_returns_prev() {
        assert_eq!(combine("", Some("my-prompt")), "my-prompt");
    }

    #[test]
    fn combine_both_empty_returns_empty() {
        assert_eq!(combine("", None), "");
        assert_eq!(combine("", Some("")), "");
    }

    #[test]
    fn combine_trims_trailing_newline_from_prev() {
        assert_eq!(combine("AH", Some("my-prompt\n")), "AH | my-prompt");
    }

    #[test]
    fn combine_trims_trailing_whitespace_from_prev() {
        assert_eq!(combine("AH", Some("my-prompt  ")), "AH | my-prompt");
    }

    // ── Task 5: previous_command_from_state() unit tests ────────────────────

    #[test]
    fn previous_command_from_state_no_state_returns_none() {
        let _g = lock();
        let home = tempfile::tempdir().unwrap();
        // Empty tempdir — no install-state.json
        let result = previous_command_from_state(home.path());
        assert!(
            result.is_none(),
            "expected None for missing state, got {result:?}"
        );
    }

    #[test]
    fn previous_command_from_state_foreign_previous_returns_some() {
        let _g = lock();
        let home = tempfile::tempdir().unwrap();

        let st = InstallState {
            claude: ClaudeState {
                statusline: Some(ClaudeStatuslineState {
                    previous: Some(
                        serde_json::json!({"type": "command", "command": "my-prompt --x"}),
                    ),
                    installed_command: "\"C:\\p\\aho.exe\" statusline".into(),
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        save(home.path(), &st).unwrap();

        let result = previous_command_from_state(home.path());
        assert_eq!(result, Some("my-prompt --x".to_string()));
    }

    #[test]
    fn previous_command_from_state_self_reference_returns_none() {
        let _g = lock();
        let home = tempfile::tempdir().unwrap();

        let st = InstallState {
            claude: ClaudeState {
                statusline: Some(ClaudeStatuslineState {
                    previous: Some(serde_json::json!({
                        "type": "command",
                        "command": "\"C:\\p\\aho.exe\" statusline"
                    })),
                    installed_command: "\"C:\\p\\aho.exe\" statusline".into(),
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        save(home.path(), &st).unwrap();

        let result = previous_command_from_state(home.path());
        assert!(
            result.is_none(),
            "self-reference must be blocked, got {result:?}"
        );
    }

    // ── Task 5: run_previous() spawn smoke test ──────────────────────────────

    #[test]
    fn run_previous_captures_stdout_from_echo() {
        #[cfg(windows)]
        {
            // `cmd /C echo hi` emits "hi\r\n" — trim to compare safely.
            let result = run_previous("echo hi", "");
            let got = result.expect("echo should succeed on Windows");
            assert!(got.trim() == "hi", "expected 'hi', got {got:?}");
        }
        #[cfg(not(windows))]
        {
            let result = run_previous("echo hi", "");
            let got = result.expect("echo should succeed on Unix");
            assert_eq!(got.trim(), "hi");
        }
    }

    #[test]
    fn run_previous_returns_none_on_bad_command() {
        // A command that definitely doesn't exist.
        let result = run_previous("__nonexistent_cmd_xyz_123__", "");
        // On Windows cmd /C returns non-zero for unknown commands; on Unix sh -c also fails.
        // Either spawn fails or exit is non-zero — both yield None.
        assert!(result.is_none() || result.as_deref().unwrap_or("").is_empty());
    }
}
