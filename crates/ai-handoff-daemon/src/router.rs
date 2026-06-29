use crate::{
    dedupe::{dedupe_key, Deduper},
    store::{find_pending, mark_consumed, save_capsule},
};
use ai_handoff_core::{
    capsule::{
        new_capsule_id, AgentKind, Capsule, Consumption, ConsumptionState, FileChange,
        RedactionMeta, Session, Summary,
    },
    config,
    fingerprint::fingerprint,
    hook_event::{normalize, HookEventKind},
    redaction::redact,
    sensor::used_percent_from_jsonl,
    trigger::{evaluate_trigger, TriggerMode},
};
use ai_handoff_ipc::{
    protocol::{degraded, Response, Status, VERSION},
    server::Handler,
};
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use std::sync::Mutex;

pub struct Router {
    deduper: Mutex<Deduper>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            deduper: Mutex::new(Deduper::new(1024)),
        }
    }

    fn ok(
        req: &ai_handoff_ipc::protocol::Request,
        hook_stdout: Value,
        diagnostics: Value,
    ) -> Response {
        Response {
            version: VERSION,
            request_id: req.request_id.clone(),
            status: Status::Ok,
            hook_stdout,
            warnings: vec![],
            diagnostics,
        }
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

impl Handler for Router {
    fn handle(&self, req: &ai_handoff_ipc::protocol::Request) -> Response {
        if req.kind == "ping" {
            return Self::ok(req, json!({ "pong": true }), json!({}));
        }
        if req.kind == "checkpoint" {
            return handle_checkpoint(req);
        }
        if req.kind != "hook_event" {
            return degraded(&req.request_id, "unsupported_request");
        }

        let key = dedupe_key(req);
        let duplicate = self
            .deduper
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .check_and_record(&key);
        if duplicate {
            return Self::ok(req, json!({}), json!({ "deduped": true }));
        }

        let Some(agent) = parse_agent(&req.agent) else {
            return degraded(&req.request_id, "daemon_error");
        };
        let Some(event) = HookEventKind::parse(&req.event) else {
            return degraded(&req.request_id, "daemon_error");
        };

        let raw = raw_with_request_fallbacks(req);
        let normalized = normalize(agent, event, &raw);
        let project_id = fingerprint(&normalized.cwd);

        match event {
            HookEventKind::SessionStart | HookEventKind::UserPromptSubmit => {
                if let Some(capsule) = find_pending(&project_id) {
                    if capsule.target_agent == normalized.agent {
                        let context = render_capsule_context(&capsule);
                        let _ = mark_consumed(
                            &project_id,
                            &capsule.capsule_id,
                            normalized.agent.clone(),
                            Utc::now(),
                        );
                        return Self::ok(
                            req,
                            json!({
                                "hookSpecificOutput": {
                                    "hookEventName": hook_event_name(event),
                                    "additionalContext": context,
                                }
                            }),
                            json!({}),
                        );
                    }
                }
                Self::ok(req, json!({}), json!({}))
            }
            HookEventKind::PostToolUse => {
                let used = normalized
                    .transcript_path
                    .as_deref()
                    .and_then(used_percent_from_jsonl);
                let cfg = ai_handoff_core::config::load();
                let resolved = ai_handoff_core::config::resolve(&cfg, &project_id);
                let mode = if resolved.enabled {
                    resolved.mode
                } else {
                    TriggerMode::Off
                };
                let outcome = evaluate_trigger(
                    used,
                    resolved.threshold,
                    mode,
                    false, // trigger dedupe: SP4d
                    &[],   // burn-rate samples: SP4d
                    &resolved.burn,
                );
                Self::ok(
                    req,
                    json!({}),
                    json!({ "used_percent": used, "trigger_reason": outcome.reason }),
                )
            }
            HookEventKind::Stop => {
                if let Some(payload) = extract_capsule_payload(&normalized.raw) {
                    let capsule = build_capsule(&payload, &project_id, &normalized);
                    let _ = save_capsule(&capsule);
                }
                Self::ok(req, json!({}), json!({}))
            }
        }
    }
}

fn handle_checkpoint(req: &ai_handoff_ipc::protocol::Request) -> Response {
    let raw = raw_with_request_fallbacks(req);
    let cwd = raw
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(&req.cwd));
    let project_id = fingerprint(&cwd);
    let agent = parse_agent(&req.agent).unwrap_or(AgentKind::Codex);
    let now = Utc::now();
    let message = raw
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("manual checkpoint")
        .to_string();
    let mut payload = raw.clone();
    if let Some(obj) = payload.as_object_mut() {
        if obj.get("goal").is_none()
            && obj
                .get("summary")
                .and_then(Value::as_object)
                .is_none_or(|summary| summary.get("goal").is_none())
        {
            obj.insert("goal".to_string(), json!(message));
        }
    }
    let normalized = normalize(agent.clone(), HookEventKind::Stop, &payload);
    let mut capsule = build_capsule(&payload, &project_id, &normalized);
    capsule.capsule_id = new_capsule_id(now);
    capsule.created_at = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    capsule.session.session_id = req.session_id.clone();

    match save_capsule(&capsule) {
        Ok(path) => Router::ok(
            req,
            json!({ "saved": true, "path": path.to_string_lossy() }),
            json!({}),
        ),
        Err(_) => degraded(&req.request_id, "daemon_error"),
    }
}

fn parse_agent(value: &str) -> Option<AgentKind> {
    match value {
        "claude-code" | "claude" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::Codex),
        _ => None,
    }
}

fn raw_with_request_fallbacks(req: &ai_handoff_ipc::protocol::Request) -> Value {
    let mut raw = if req.raw_hook_input.is_object() {
        req.raw_hook_input.clone()
    } else {
        json!({})
    };

    if let Some(obj) = raw.as_object_mut() {
        obj.entry("cwd").or_insert_with(|| json!(req.cwd));
        if let Some(session_id) = &req.session_id {
            obj.entry("session_id")
                .or_insert_with(|| json!(session_id.clone()));
        }
        if let Some(turn_id) = &req.turn_id {
            obj.entry("turn_id")
                .or_insert_with(|| json!(turn_id.clone()));
        }
    }
    raw
}

fn hook_event_name(event: HookEventKind) -> &'static str {
    match event {
        HookEventKind::SessionStart => "SessionStart",
        HookEventKind::UserPromptSubmit => "UserPromptSubmit",
        HookEventKind::PostToolUse => "PostToolUse",
        HookEventKind::Stop => "Stop",
    }
}

fn render_capsule_context(capsule: &Capsule) -> String {
    let mut lines = vec![
        "[CURRENT HANDOFF]".to_string(),
        format!("goal: {}", capsule.summary.goal),
    ];
    if !capsule.summary.done.is_empty() {
        lines.push(format!("done: {}", capsule.summary.done.join("; ")));
    }
    if !capsule.summary.remaining.is_empty() {
        lines.push(format!(
            "remaining: {}",
            capsule.summary.remaining.join("; ")
        ));
    }
    if let Some(next) = &capsule.next_prompt {
        lines.push(format!("next_prompt: {next}"));
    }
    lines.join("\n")
}

fn extract_capsule_payload(raw: &Value) -> Option<Value> {
    let text = [
        "last_assistant_message",
        "final_answer",
        "message",
        "content",
    ]
    .iter()
    .find_map(|key| raw.get(*key).and_then(Value::as_str))?;
    let marker = "```ai-handoff-capsule";
    let start = text.find(marker)? + marker.len();
    let after_marker = &text[start..];
    let content_start = after_marker.find('\n').map(|idx| idx + 1).unwrap_or(0);
    let content = &after_marker[content_start..];
    let end = content.find("```")?;
    serde_json::from_str(content[..end].trim()).ok()
}

fn build_capsule(
    payload: &Value,
    project_id: &str,
    event: &ai_handoff_core::hook_event::NormalizedHookEvent,
) -> Capsule {
    let now = Utc::now();
    let summary_value = payload.get("summary").unwrap_or(payload);
    let mut redacted = false;
    let limits = config::load().capsule;

    let goal = redact_string(
        string_field(summary_value, "goal").unwrap_or_else(|| "handoff capsule".to_string()),
        &mut redacted,
    );
    let done = limit_items(
        redact_strings(
            array_field(summary_value, &["done", "completed"]),
            &mut redacted,
        ),
        limits.done_limit(),
    );
    let remaining = limit_items(
        redact_strings(
            array_field(summary_value, &["remaining", "next_actions"]),
            &mut redacted,
        ),
        limits.remaining_limit(),
    );
    let risks = limit_items(
        redact_strings(
            array_field(summary_value, &["risks", "open_issues"]),
            &mut redacted,
        ),
        limits.risks_limit(),
    );
    let next_prompt = string_field(payload, "next_prompt")
        .map(|value| redact_string(value, &mut redacted))
        .map(|value| limit_next_prompt(value, limits.next_prompt_limit()));

    Capsule {
        schema_version: 2,
        capsule_id: new_capsule_id(now),
        project_id: project_id.to_string(),
        created_at: now.to_rfc3339_opts(SecondsFormat::Secs, true),
        source_agent: event.agent.clone(),
        target_agent: opposite_agent(&event.agent),
        session: Session {
            session_id: event.session_id.clone(),
            ..Session::default()
        },
        summary: Summary {
            goal,
            done,
            remaining,
            risks,
        },
        files: file_changes(payload),
        next_prompt,
        redaction: RedactionMeta {
            applied: redacted,
            ruleset: "default-v2".to_string(),
        },
        consumption: Consumption {
            state: ConsumptionState::Pending,
            consumed_by: None,
            consumed_at: None,
        },
    }
}

fn limit_items(mut items: Vec<String>, limit: usize) -> Vec<String> {
    items.truncate(limit);
    items
}

fn limit_next_prompt(value: String, limit: usize) -> String {
    let items = value
        .split(['|', '\n'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .take(limit)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if items.len() <= 1 {
        value
    } else {
        items.join(" | ")
    }
}

fn opposite_agent(agent: &AgentKind) -> AgentKind {
    match agent {
        AgentKind::ClaudeCode => AgentKind::Codex,
        AgentKind::Codex => AgentKind::ClaudeCode,
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn array_field(value: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_array))
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn redact_string(value: String, hit: &mut bool) -> String {
    let (out, redacted) = redact(&value);
    *hit |= redacted;
    out
}

fn redact_strings(values: Vec<String>, hit: &mut bool) -> Vec<String> {
    values
        .into_iter()
        .map(|value| redact_string(value, hit))
        .collect()
}

fn file_changes(payload: &Value) -> Vec<FileChange> {
    payload
        .get("files")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .filter_map(|file| {
                    Some(FileChange {
                        path: file.get("path")?.as_str()?.to_string(),
                        status: file
                            .get("status")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        summary: file
                            .get("summary")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use ai_handoff_core::{
        capsule::{
            AgentKind, Capsule, Consumption, ConsumptionState, RedactionMeta, Session, Summary,
        },
        fingerprint::fingerprint,
    };
    use ai_handoff_ipc::{
        protocol::{ClientInfo, Request, Status, VERSION},
        server::Handler,
    };
    use serde_json::json;

    fn request(
        id: &str,
        event: &str,
        agent: &str,
        cwd: &std::path::Path,
        raw: serde_json::Value,
    ) -> Request {
        Request {
            version: VERSION,
            request_id: id.into(),
            kind: "hook_event".into(),
            agent: agent.into(),
            event: event.into(),
            received_at: "2026-06-25T12:34:56Z".into(),
            cwd: cwd.to_string_lossy().into_owned(),
            session_id: Some("s1".into()),
            turn_id: Some(id.into()),
            raw_hook_input: raw,
            client: ClientInfo {
                binary_version: "2.0.0-mvp".into(),
                pid: 1,
                platform: "windows".into(),
            },
        }
    }

    fn pending_capsule(project_id: &str) -> Capsule {
        Capsule {
            schema_version: 2,
            capsule_id: "cap_20260625_120000_abcd".into(),
            project_id: project_id.into(),
            created_at: "2026-06-25T12:00:00Z".into(),
            source_agent: AgentKind::Codex,
            target_agent: AgentKind::ClaudeCode,
            session: Session::default(),
            summary: Summary {
                goal: "continue router".into(),
                done: vec!["core".into()],
                remaining: vec!["ipc".into()],
                risks: vec![],
            },
            files: vec![],
            next_prompt: Some("pick up".into()),
            redaction: RedactionMeta {
                applied: true,
                ruleset: "default-v2".into(),
            },
            consumption: Consumption {
                state: ConsumptionState::Pending,
                consumed_by: None,
                consumed_at: None,
            },
        }
    }

    #[test]
    fn stop_with_fenced_capsule_writes_pending_capsule() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let router = Router::new();
        let req = request(
            "turn-stop",
            "stop",
            "codex",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "last_assistant_message": "done\n```ai-handoff-capsule\n{\"goal\":\"ship MVP\",\"remaining\":[\"daemon\"],\"next_prompt\":\"continue\"}\n```"
            }),
        );

        let resp = router.handle(&req);
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(resp.hook_stdout, json!({}));
        let project_id = fingerprint(cwd.path());
        let pending = crate::store::find_pending(&project_id).unwrap();
        assert_eq!(pending.summary.goal, "ship MVP");
        assert_eq!(pending.source_agent, AgentKind::Codex);
        assert_eq!(pending.target_agent, AgentKind::ClaudeCode);
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn stop_capsule_respects_configured_summary_item_limits() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[capsule]\nremaining_max_items = 2\ndone_max_items = 1\nrisks_max_items = 1\nnext_prompt_max_items = 2\n",
        )
        .unwrap();
        let router = Router::new();
        let req = request(
            "turn-stop",
            "stop",
            "codex",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "last_assistant_message": "done\n```ai-handoff-capsule\n{\"goal\":\"ship MVP\",\"done\":[\"a\",\"b\"],\"remaining\":[\"c\",\"d\",\"e\"],\"risks\":[\"f\",\"g\"],\"next_prompt\":\"one | two | three\"}\n```"
            }),
        );

        let resp = router.handle(&req);
        assert_eq!(resp.status, Status::Ok);
        let project_id = fingerprint(cwd.path());
        let pending = crate::store::find_pending(&project_id).unwrap();
        assert_eq!(pending.summary.done, vec!["a"]);
        assert_eq!(pending.summary.remaining, vec!["c", "d"]);
        assert_eq!(pending.summary.risks, vec!["f"]);
        assert_eq!(pending.next_prompt.as_deref(), Some("one | two"));
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn session_start_injects_and_consumes_pending_capsule() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());
        crate::store::save_capsule(&pending_capsule(&project_id)).unwrap();

        let router = Router::new();
        let req = request(
            "turn-start",
            "session-start",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        let resp = router.handle(&req);
        assert_eq!(resp.status, Status::Ok);
        assert!(resp.hook_stdout["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("continue router"));
        assert!(crate::store::find_pending(&project_id).is_none());
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn session_start_with_no_pending_returns_empty_stdout() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let router = Router::new();
        let req = request(
            "turn-empty",
            "session-start",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        let resp = router.handle(&req);
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(resp.hook_stdout, json!({}));
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn duplicate_request_is_noop() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let router = Router::new();
        let req = request(
            "turn-dupe",
            "stop",
            "codex",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "last_assistant_message": "```ai-handoff-capsule\n{\"goal\":\"once\"}\n```"
            }),
        );
        router.handle(&req);
        let second = router.handle(&req);
        assert_eq!(second.hook_stdout, json!({}));
        let project_id = fingerprint(cwd.path());
        let count = std::fs::read_dir(ai_handoff_core::paths::project_dir(&project_id))
            .unwrap()
            .count();
        assert_eq!(count, 1);
        std::env::remove_var("AI_HANDOFF_HOME");
    }
}
