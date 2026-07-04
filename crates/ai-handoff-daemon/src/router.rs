use crate::{
    dedupe::{dedupe_key, Deduper},
    store::{find_pending, mark_consumed, save_capsule, save_project_label},
    trigger_mark,
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
    sensor::{claude_trigger_usage, codex_trigger_usage_from_jsonl},
    trigger::{evaluate_trigger, TriggerAction, TriggerMode},
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
    /// Once-per-session marks: threshold-trigger firings and pending-capsule
    /// notices, keyed by `<kind>:<agent>:<session_id-or-project_id>`.
    session_marks: Mutex<Deduper>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            deduper: Mutex::new(Deduper::new(1024)),
            session_marks: Mutex::new(Deduper::new(1024)),
        }
    }

    /// Record a once-per-session mark. Returns `true` when the mark was
    /// already present (i.e. the action already happened this session).
    fn mark_seen(&self, key: &str) -> bool {
        self.session_marks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .check_and_record(key)
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
        if req.kind == "handoff_consume" {
            return handle_handoff_consume(req);
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
                // A pending capsule is only announced, never consumed here —
                // consumption is explicit via `ai-handoff handoff` (/handoff).
                if let Some(capsule) = find_pending(&project_id) {
                    if capsule.target_agent == normalized.agent {
                        let mark = session_mark_key("notice", &req.agent, &normalized, &project_id);
                        if !self.mark_seen(&mark) {
                            return Self::ok(
                                req,
                                json!({
                                    "hookSpecificOutput": {
                                        "hookEventName": hook_event_name(event),
                                        "additionalContext": render_pending_notice(&capsule),
                                    }
                                }),
                                json!({ "pending_notice": true }),
                            );
                        }
                    }
                }
                Self::ok(req, json!({}), json!({}))
            }
            HookEventKind::PostToolUse => {
                let now_ms = Utc::now().timestamp_millis();
                let usage = match normalized.agent {
                    // Claude usage comes from statusline samples (a sample
                    // whose 5h window is still open is a valid lower bound);
                    // the Claude transcript JSONL has no rate-limit payload.
                    AgentKind::ClaudeCode => claude_trigger_usage(now_ms),
                    AgentKind::Codex => normalized
                        .transcript_path
                        .as_deref()
                        .and_then(|path| codex_trigger_usage_from_jsonl(path, now_ms)),
                };
                let used = usage.map(|sample| sample.used_percent);
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
                    false,
                    &[], // burn-rate samples: SP4d
                    &resolved.burn,
                );
                let mut fired = false;
                let mut suppressed = false;
                let mut trigger_expires_at_ms = None;
                let stdout = match outcome.action {
                    TriggerAction::None => json!({}),
                    action => {
                        let mark = trigger_mark::check_and_record(
                            &normalized.agent,
                            now_ms,
                            usage.and_then(|sample| sample.resets_at_ms),
                        );
                        trigger_expires_at_ms = Some(mark.expires_at_ms);
                        if !mark.fired {
                            suppressed = true;
                            json!({})
                        } else {
                            fired = true;
                            let context = render_trigger_context(
                                action,
                                used.unwrap_or_default(),
                                resolved.threshold,
                                &req.agent,
                                cfg.capsule.language,
                            );
                            json!({
                                "decision": "block",
                                "reason": trigger_block_reason(action),
                                "hookSpecificOutput": {
                                    "hookEventName": "PostToolUse",
                                    "additionalContext": context,
                                }
                            })
                        }
                    }
                };
                Self::ok(
                    req,
                    stdout,
                    json!({
                        "used_percent": used,
                        "trigger_reason": outcome.reason,
                        "trigger_fired": fired,
                        "trigger_suppressed": suppressed,
                        "trigger_expires_at_ms": trigger_expires_at_ms,
                    }),
                )
            }
            HookEventKind::Stop => {
                if let Some(payload) = extract_capsule_payload(&normalized.raw) {
                    let capsule = build_capsule(&payload, &project_id, &normalized);
                    let _ = save_project_label(&project_id, &normalized.cwd);
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

    let _ = save_project_label(&project_id, &cwd);
    match save_capsule(&capsule) {
        Ok(path) => Router::ok(
            req,
            json!({ "saved": true, "path": path.to_string_lossy() }),
            json!({}),
        ),
        Err(_) => degraded(&req.request_id, "daemon_error"),
    }
}

/// Explicit capsule consumption (`ai-handoff handoff` / the /handoff skill).
/// Finds the pending capsule targeted at the calling agent, marks it consumed,
/// and returns its rendered context. Empty stdout when nothing is pending.
fn handle_handoff_consume(req: &ai_handoff_ipc::protocol::Request) -> Response {
    let raw = raw_with_request_fallbacks(req);
    let cwd = raw
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(&req.cwd));
    let project_id = fingerprint(&cwd);
    let Some(agent) = parse_agent(&req.agent) else {
        return degraded(&req.request_id, "daemon_error");
    };

    match find_pending(&project_id) {
        Some(capsule) if capsule.target_agent == agent => {
            let context = render_capsule_context(&capsule);
            if mark_consumed(&project_id, &capsule.capsule_id, agent, Utc::now()).is_err() {
                return degraded(&req.request_id, "daemon_error");
            }
            Router::ok(
                req,
                json!({
                    "hookSpecificOutput": {
                        "hookEventName": "Handoff",
                        "additionalContext": context,
                    },
                    "consumed": true,
                    "capsule_id": capsule.capsule_id,
                }),
                json!({}),
            )
        }
        _ => Router::ok(req, json!({}), json!({ "pending": false })),
    }
}

/// Once-per-session mark key. Falls back to the project fingerprint when the
/// hook payload carries no session id.
fn session_mark_key(
    kind: &str,
    agent: &str,
    normalized: &ai_handoff_core::hook_event::NormalizedHookEvent,
    project_id: &str,
) -> String {
    format!(
        "{kind}:{agent}:{}",
        normalized.session_id.as_deref().unwrap_or(project_id)
    )
}

fn agent_cli_name(agent: &str) -> &str {
    match agent {
        "claude" | "claude-code" => "claude-code",
        _ => "codex",
    }
}

fn trigger_block_reason(action: TriggerAction) -> &'static str {
    match action {
        TriggerAction::Create => {
            "ai-handoff five-hour usage threshold reached; create a checkpoint before continuing"
        }
        _ => "ai-handoff five-hour usage threshold reached; ask the user before continuing",
    }
}

/// Context injected when the five-hour trigger fires. Instructs the agent to
/// checkpoint (auto) or ask the user first (ask), then resume the interrupted
/// work — the capsule must never end the turn.
fn render_trigger_context(
    action: TriggerAction,
    used_percent: f64,
    threshold: f64,
    agent: &str,
    language: config::Language,
) -> String {
    let agent = agent_cli_name(agent);
    let copy = trigger_prompt_copy(language);
    let header = format!(
        "[ai-handoff] Five-hour usage {used_percent:.0}% reached the configured threshold {threshold:.0}%."
    );
    let checkpoint_steps = format!(
        "Write a small JSON file summarizing the CURRENT work (fields: goal, done[], remaining[], risks[], next_prompt), then run:\n  ai-handoff checkpoint --agent {agent} --file <path-to.json>\nAfter the checkpoint succeeds, resume the interrupted work exactly where it stopped."
    );
    match action {
        TriggerAction::Create => format!(
            "{header}\nCreate a handoff capsule NOW without asking the user. {checkpoint_steps}"
        ),
        _ => render_trigger_question_context(agent, &header, &checkpoint_steps, copy),
    }
}

/// Agent-specific question instructions for ask mode.
fn render_trigger_question_context(
    agent: &str,
    header: &str,
    checkpoint_steps: &str,
    copy: TriggerPromptCopy,
) -> String {
    if agent == "claude-code" {
        format!(
            "{header}\nUse AskUserQuestion now. Question: \"{}\"\nOptions:\n- {}: {}\n- {}: {}\n- {}: {}\nIf the user selects {}, ask one follow-up chat question for their free-text instruction before deciding whether to create the capsule. Then follow that instruction.\nFor {}: {checkpoint_steps}\nFor {}: resume the interrupted work without creating a capsule.\nAfter the selected path finishes, resume the interrupted work exactly where it stopped.",
            copy.question,
            copy.yes,
            copy.yes_desc,
            copy.no,
            copy.no_desc,
            copy.other,
            copy.other_desc,
            copy.other,
            copy.yes,
            copy.no,
        )
    } else {
        format!(
            "{header}\nAsk the user in plain chat and wait for the answer: \"{}\"\nOptions:\n- {}: {}\n- {}: {}\n- {}: {}\nIf the user chooses {}, ask one follow-up chat question for their free-text instruction before deciding whether to create the capsule. Then follow that instruction.\nFor {}: {checkpoint_steps}\nFor {}: resume the interrupted work without creating a capsule.\nAfter the selected path finishes, resume the interrupted work exactly where it stopped.",
            copy.question,
            copy.yes,
            copy.yes_desc,
            copy.no,
            copy.no_desc,
            copy.other,
            copy.other_desc,
            copy.other,
            copy.yes,
            copy.no,
        )
    }
}

#[derive(Clone, Copy)]
struct TriggerPromptCopy {
    question: &'static str,
    yes: &'static str,
    no: &'static str,
    other: &'static str,
    yes_desc: &'static str,
    no_desc: &'static str,
    other_desc: &'static str,
}

fn trigger_prompt_copy(language: config::Language) -> TriggerPromptCopy {
    match language {
        config::Language::Ko => TriggerPromptCopy {
            question: "5시간 한도 임계치에 도달했습니다. 캡슐을 저장하시겠습니까?",
            yes: "네",
            no: "아니오",
            other: "기타",
            yes_desc: "캡슐 JSON을 작성하고 checkpoint를 실행한 뒤 원래 작업을 계속합니다.",
            no_desc: "캡슐을 만들지 않고 원래 작업을 계속합니다.",
            other_desc: "사용자의 추가 지시에 따라 캡슐 생성 여부를 정한 뒤 원래 작업을 계속합니다.",
        },
        config::Language::Ja => TriggerPromptCopy {
            question: "5時間制限のしきい値に達しました。カプセルを保存しますか？",
            yes: "はい",
            no: "いいえ",
            other: "その他",
            yes_desc: "カプセルJSONを作成してcheckpointを実行し、元の作業を続けます。",
            no_desc: "カプセルを作成せず、元の作業を続けます。",
            other_desc: "ユーザーの追加指示に従ってカプセル作成の有無を決め、元の作業を続けます。",
        },
        config::Language::En => TriggerPromptCopy {
            question: "The five-hour usage threshold was reached. Save a handoff capsule?",
            yes: "Yes",
            no: "No",
            other: "Other",
            yes_desc: "write the capsule JSON, run checkpoint, then continue the original work.",
            no_desc: "continue the original work without creating a capsule.",
            other_desc: "ask for the user's custom instruction, then create or skip the capsule accordingly.",
        },
    }
}

/// Context injected on SessionStart / UserPromptSubmit when a pending capsule
/// targets this agent. Announce only — /handoff performs the consumption.
fn render_pending_notice(capsule: &Capsule) -> String {
    format!(
        "[ai-handoff] A pending handoff capsule for this project targets you (goal: {}). It was NOT consumed automatically. Briefly tell the user it exists; run /handoff (ai-handoff handoff) only when the user wants to continue from it.",
        capsule.summary.goal
    )
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
    fn session_start_notifies_without_consuming_pending_capsule() {
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
            json!({ "cwd": cwd.path().to_string_lossy(), "session_id": "s-notice" }),
        );
        let resp = router.handle(&req);
        assert_eq!(resp.status, Status::Ok);
        let context = resp.hook_stdout["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(context.contains("continue router"));
        assert!(context.contains("/handoff"));
        // The capsule stays pending — only /handoff consumes it.
        assert!(crate::store::find_pending(&project_id).is_some());

        // The notice fires once per session, not on every prompt.
        let again = request(
            "turn-start-2",
            "user-prompt",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy(), "session_id": "s-notice" }),
        );
        let resp2 = router.handle(&again);
        assert_eq!(resp2.hook_stdout, json!({}));
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_consume_marks_consumed_and_returns_context() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());
        crate::store::save_capsule(&pending_capsule(&project_id)).unwrap();

        let router = Router::new();
        let mut req = request(
            "turn-consume",
            "handoff",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        req.kind = "handoff_consume".into();
        let resp = router.handle(&req);
        assert_eq!(resp.status, Status::Ok);
        assert!(resp.hook_stdout["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("continue router"));
        assert_eq!(resp.hook_stdout["consumed"], true);
        assert!(crate::store::find_pending(&project_id).is_none());
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_consume_ignores_capsule_for_other_agent() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());
        // Capsule targets ClaudeCode; Codex asks to consume — nothing happens.
        crate::store::save_capsule(&pending_capsule(&project_id)).unwrap();

        let router = Router::new();
        let mut req = request(
            "turn-consume-wrong",
            "handoff",
            "codex",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        req.kind = "handoff_consume".into();
        let resp = router.handle(&req);
        assert_eq!(resp.hook_stdout, json!({}));
        assert!(crate::store::find_pending(&project_id).is_some());
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn post_tool_use_fires_claude_trigger_once_per_session() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[triggers.five_hour]\nenabled = true\nthreshold_percent = 50\nmode = \"ask\"\n[capsule]\nlanguage = \"ko\"\n",
        )
        .unwrap();
        // Fresh Claude statusline sample above threshold.
        let now_ms = Utc::now().timestamp_millis();
        assert!(ai_handoff_core::sensor::record_claude_rate_limit(
            &json!({
                "session_id": "sid-trigger",
                "rate_limits": { "five_hour": { "used_percentage": 75.0 } }
            }),
            now_ms,
        ));

        let router = Router::new();
        let req = request(
            "turn-trigger",
            "post-tool-use",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy(), "session_id": "s-trig" }),
        );
        let resp = router.handle(&req);
        assert_eq!(resp.status, Status::Ok);
        let context = resp.hook_stdout["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("trigger context");
        assert_eq!(resp.hook_stdout["decision"], "block");
        assert!(resp.hook_stdout["reason"]
            .as_str()
            .unwrap()
            .contains("five-hour usage threshold"));
        assert!(context.contains("AskUserQuestion"));
        assert!(context.contains("네"));
        assert!(context.contains("아니오"));
        assert!(context.contains("기타"));
        assert!(context.contains("ai-handoff checkpoint --agent claude-code"));
        assert!(context.contains("resume the interrupted work"));
        assert_eq!(resp.diagnostics["trigger_fired"], true);
        assert_eq!(resp.diagnostics["trigger_suppressed"], false);
        assert!(resp.diagnostics["trigger_expires_at_ms"].as_i64().is_some());

        // Second PostToolUse in the same session: suppressed.
        let again = request(
            "turn-trigger-2",
            "post-tool-use",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy(), "session_id": "s-trig" }),
        );
        let resp2 = router.handle(&again);
        assert_eq!(resp2.hook_stdout, json!({}));
        assert_eq!(resp2.diagnostics["trigger_fired"], false);
        assert_eq!(resp2.diagnostics["trigger_suppressed"], true);
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn post_tool_use_trigger_mark_survives_new_router_instance_for_codex() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[triggers.five_hour]\nenabled = true\nthreshold_percent = 50\nmode = \"ask\"\n[capsule]\nlanguage = \"en\"\n",
        )
        .unwrap();
        let transcript = home.path().join("codex.jsonl");
        std::fs::write(
            &transcript,
            "{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":75.0,\"resets_at\":4102444800}}}}\n",
        )
        .unwrap();

        let req = request(
            "turn-codex-trigger",
            "post-tool-use",
            "codex",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "session_id": "s-codex-a",
                "transcript_path": transcript.to_string_lossy()
            }),
        );
        let first = Router::new().handle(&req);
        let context = first.hook_stdout["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("codex context");
        assert!(context.contains("Ask the user in plain chat"));
        assert!(!context.contains("AskUserQuestion"));
        assert_eq!(first.diagnostics["trigger_fired"], true);

        let second = request(
            "turn-codex-trigger-2",
            "post-tool-use",
            "codex",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "session_id": "s-codex-b",
                "transcript_path": transcript.to_string_lossy()
            }),
        );
        let suppressed = Router::new().handle(&second);
        assert_eq!(suppressed.hook_stdout, json!({}));
        assert_eq!(suppressed.diagnostics["trigger_fired"], false);
        assert_eq!(suppressed.diagnostics["trigger_suppressed"], true);
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn post_tool_use_auto_mode_instructs_checkpoint_without_asking() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[triggers.five_hour]\nenabled = true\nthreshold_percent = 50\nmode = \"auto\"\n",
        )
        .unwrap();
        let now_ms = Utc::now().timestamp_millis();
        assert!(ai_handoff_core::sensor::record_claude_rate_limit(
            &json!({
                "session_id": "sid-auto",
                "rate_limits": { "five_hour": { "used_percentage": 90.0 } }
            }),
            now_ms,
        ));

        let router = Router::new();
        let req = request(
            "turn-auto",
            "post-tool-use",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy(), "session_id": "s-auto" }),
        );
        let resp = router.handle(&req);
        let context = resp.hook_stdout["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("trigger context");
        assert!(context.contains("without asking"));
        assert!(context.contains("ai-handoff checkpoint --agent claude-code"));
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
            .filter(|entry| {
                entry.as_ref().ok().is_some_and(|entry| {
                    entry
                        .path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext == "json")
                })
            })
            .count();
        assert_eq!(count, 1);
        std::env::remove_var("AI_HANDOFF_HOME");
    }
}
