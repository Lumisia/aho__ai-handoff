use crate::{
    dedupe::{dedupe_key, Deduper},
    store::{
        find_pending, list_pending, mark_consumed, pending_for, save_capsule, save_project_label,
    },
    trigger_mark,
};
use ai_handoff_core::{
    account, account_api,
    capsule::{
        canonical_agent_id, new_capsule_id, AgentKind, Capsule, Consumption, ConsumptionState,
        FileChange, RedactionMeta, Session, Summary,
    },
    config,
    fingerprint::fingerprint,
    hook_event::{normalize, HookEventKind},
    redaction::redact,
    sensor::{
        claude_trigger_usage, claude_trigger_usage_from_raw, codex_sessions_dirs,
        resolve_codex_trigger_usage, TriggerUsage,
    },
    trigger::{evaluate_trigger, TriggerAction, TriggerMode},
};
use ai_handoff_ipc::{
    protocol::{degraded, Response, Status, VERSION},
    server::Handler,
};
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Router {
    deduper: Mutex<Deduper>,
    /// Once-per-session marks: threshold-trigger firings and pending-capsule
    /// notices, keyed by `<kind>:<agent>:<session_id-or-project_id>`.
    session_marks: Mutex<Deduper>,
    /// Resolved Codex rollout files by session id, so the sessions-directory
    /// walk runs at most once per session instead of on every hook event.
    codex_rollouts: Mutex<HashMap<String, PathBuf>>,
    /// TTL-cached provider usage per agent ("codex"/"claude"), so the hook-time
    /// fallback fetch hits the network at most once per [`PROVIDER_USAGE_TTL_MS`]
    /// rather than on every tool use.
    provider_usage: Mutex<HashMap<&'static str, ProviderUsageEntry>>,
}

/// One cached provider-usage reading (or a cached miss) for an agent.
#[derive(Clone, Copy)]
struct ProviderUsageEntry {
    usage: Option<TriggerUsage>,
    fetched_at_ms: i64,
}

/// How long a hook-time provider-usage fetch (or its failure) is reused before
/// the next hook event refetches. Five-hour limits move slowly, so a few
/// minutes keeps the trigger responsive without per-tool network calls.
const PROVIDER_USAGE_TTL_MS: i64 = 3 * 60 * 1000;

impl Router {
    pub fn new() -> Self {
        Self {
            deduper: Mutex::new(Deduper::new(1024)),
            session_marks: Mutex::new(Deduper::new(1024)),
            codex_rollouts: Mutex::new(HashMap::new()),
            provider_usage: Mutex::new(HashMap::new()),
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
        if req.kind == "handoff_peek" {
            return handle_handoff_peek(req);
        }
        if req.kind == "handoff_retarget" {
            return handle_handoff_retarget(req);
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
                if let Some(capsule) =
                    find_pending(&project_id, normalized.agent.as_canonical_str())
                {
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
                Self::ok(req, json!({}), json!({}))
            }
            HookEventKind::PostToolUse => {
                let (stdout, diagnostics) =
                    self.evaluate_five_hour_trigger(req, &normalized, &project_id, event);
                Self::ok(req, stdout, diagnostics)
            }
            HookEventKind::Stop => {
                if let Some(payload) = extract_capsule_payload(&normalized.raw) {
                    let capsule = build_capsule(
                        &payload,
                        &project_id,
                        &normalized.cwd,
                        normalized.session_id.clone(),
                        normalized.agent.as_canonical_str(),
                    );
                    let _ = save_project_label(&project_id, &normalized.cwd);
                    let _ = save_capsule(&capsule);
                    // The turn just checkpointed — asking again would be noise.
                    return Self::ok(req, json!({}), json!({ "capsule_saved": true }));
                }
                // Claude's PostToolUse hook only matches Write|Edit|Bash, so a
                // read-heavy turn would otherwise never reach the threshold
                // check; Stop covers every turn for both agents.
                let (stdout, diagnostics) =
                    self.evaluate_five_hour_trigger(req, &normalized, &project_id, event);
                Self::ok(req, stdout, diagnostics)
            }
        }
    }
}

impl Router {
    /// Five-hour threshold evaluation shared by PostToolUse and Stop.
    /// Returns `(hook_stdout, diagnostics)`.
    fn evaluate_five_hour_trigger(
        &self,
        req: &ai_handoff_ipc::protocol::Request,
        normalized: &ai_handoff_core::hook_event::NormalizedHookEvent,
        project_id: &str,
        event: HookEventKind,
    ) -> (Value, Value) {
        let now_ms = Utc::now().timestamp_millis();
        let (usage, usage_source, usage_unknown) = match normalized.agent {
            // Claude usage comes from statusline samples (a sample whose 5h
            // window is still open is a valid lower bound); the Claude
            // transcript JSONL has no rate-limit payload.
            AgentKind::ClaudeCode => {
                if let Some(usage) = claude_trigger_usage_from_raw(&normalized.raw, now_ms) {
                    (Some(usage), Some("raw-rate-limits"), Vec::new())
                } else {
                    let usage = claude_trigger_usage(now_ms);
                    let source = usage.is_some().then_some("claude-statusline");
                    let unknown = if usage.is_none() {
                        vec!["no-raw-rate-limits", "no-fresh-statusline-sample"]
                    } else {
                        vec!["no-raw-rate-limits"]
                    };
                    (usage, source, unknown)
                }
            }
            AgentKind::Codex => {
                let session_id = normalized.session_id.as_deref();
                let cached = session_id.and_then(|sid| {
                    self.codex_rollouts
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .get(sid)
                        .cloned()
                });
                let resolution = resolve_codex_trigger_usage(
                    &normalized.raw,
                    normalized.transcript_path.as_deref(),
                    session_id,
                    &codex_sessions_dirs(),
                    cached.as_deref(),
                    now_ms,
                );
                if let (Some(sid), Some(path)) = (
                    session_id,
                    (resolution.source == Some("session-rollout"))
                        .then_some(resolution.rollout_path.as_ref())
                        .flatten(),
                ) {
                    self.codex_rollouts
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .insert(sid.to_string(), path.clone());
                }
                (
                    resolution.usage,
                    resolution.source,
                    resolution.unknown_reasons,
                )
            }
        };
        let cfg = ai_handoff_core::config::load();
        let resolved = ai_handoff_core::config::resolve(&cfg, project_id);
        let mode = if resolved.enabled {
            resolved.mode
        } else {
            TriggerMode::Off
        };

        // Local samples are the only usage the trigger normally sees, but they
        // can be missing (Claude Code's statusline never records a five-hour
        // sample) or below threshold while the account is actually over. When
        // that leaves us unable to confirm an overage, consult the ACTIVE
        // account's real usage from the provider API — hook-time only (never
        // while idle), TTL-cached, short timeout, fail-open.
        let mut usage = usage;
        let mut usage_source = usage_source;
        if should_consult_provider(mode, usage.map(|s| s.used_percent), resolved.threshold) {
            if let Some(provider) = self.provider_trigger_usage(&normalized.agent, now_ms) {
                if usage.is_none_or(|local| provider.used_percent > local.used_percent) {
                    usage_source = Some("provider-api");
                }
                usage = Some(pick_higher_usage(usage, provider));
            }
        }

        let used = usage.map(|sample| sample.used_percent);
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
                    if event == HookEventKind::Stop {
                        // Stop hooks carry no additionalContext channel; the
                        // block reason is what the agent reads to continue.
                        json!({
                            "decision": "block",
                            "reason": context,
                        })
                    } else {
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
            }
        };
        (
            stdout,
            json!({
                "used_percent": used,
                "usage_source": usage_source,
                "usage_unknown_reasons": usage_unknown,
                "trigger_reason": outcome.reason,
                "trigger_fired": fired,
                "trigger_suppressed": suppressed,
                "trigger_expires_at_ms": trigger_expires_at_ms,
            }),
        )
    }

    /// Active-account provider usage for the five-hour trigger, TTL-cached so a
    /// burst of hook events costs at most one network round-trip per window.
    /// Caches misses too, so a failed or below-threshold read does not refetch
    /// on the next tool use.
    fn provider_trigger_usage(&self, agent: &AgentKind, now_ms: i64) -> Option<TriggerUsage> {
        let (acct, key) = match agent {
            AgentKind::Codex => (account::Agent::Codex, "codex"),
            AgentKind::ClaudeCode => (account::Agent::Claude, "claude"),
        };
        {
            let cache = self
                .provider_usage
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if let Some(entry) = cache.get(key) {
                if now_ms - entry.fetched_at_ms <= PROVIDER_USAGE_TTL_MS {
                    return entry.usage;
                }
            }
        }
        // Fetch outside the lock (network I/O).
        let usage = fetch_provider_trigger_usage(acct);
        let mut cache = self
            .provider_usage
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        cache.insert(
            key,
            ProviderUsageEntry {
                usage,
                fetched_at_ms: now_ms,
            },
        );
        usage
    }
}

/// Whether to spend a provider fetch: only when the trigger is active and the
/// local sample cannot already confirm an overage (missing, or below the
/// threshold). If the local sample already meets the threshold we fire without
/// any network call.
fn should_consult_provider(mode: TriggerMode, local_used: Option<f64>, threshold: f64) -> bool {
    if matches!(mode, TriggerMode::Off) {
        return false;
    }
    match local_used {
        Some(used) => used < threshold,
        None => true,
    }
}

/// Keep whichever reading reports higher usage (usage only grows within a
/// window, so the higher number is the safer lower bound), preferring the
/// provider when there is no local sample.
fn pick_higher_usage(local: Option<TriggerUsage>, provider: TriggerUsage) -> TriggerUsage {
    match local {
        Some(local) if local.used_percent >= provider.used_percent => local,
        _ => provider,
    }
}

/// One-shot: the active slot's real five-hour usage from the provider API, or
/// `None` when there is no active slot or the (short-timeout) fetch fails.
fn fetch_provider_trigger_usage(agent: account::Agent) -> Option<TriggerUsage> {
    let active = account::list_slots(agent)
        .into_iter()
        .find(|slot| slot.active)?;
    let timeouts = account_api::FetchTimeouts {
        connect: std::time::Duration::from_secs(2),
        read: std::time::Duration::from_secs(3),
    };
    let usage = account_api::fetch_slot_usage_with(agent, &active.meta.label, timeouts).ok()?;
    let window = usage.five_hour?;
    Some(TriggerUsage {
        used_percent: window.used_percent,
        resets_at_ms: window.resets_at.map(|secs| secs * 1000),
    })
}

fn handle_checkpoint(req: &ai_handoff_ipc::protocol::Request) -> Response {
    let raw = raw_with_request_fallbacks(req);
    let cwd = raw
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(&req.cwd));
    let project_id = fingerprint(&cwd);
    // Canonicalized so any agent id (grok, gemini, ...) can checkpoint —
    // unknown ids pass through instead of being coerced to codex.
    let agent = canonical_agent_id(&req.agent).unwrap_or_else(|| "codex".to_string());
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
    let mut capsule = build_capsule(&payload, &project_id, &cwd, req.session_id.clone(), &agent);
    capsule.capsule_id = new_capsule_id(now);
    capsule.created_at = now.to_rfc3339_opts(SecondsFormat::Secs, true);

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
/// Claims the newest pending capsule addressed to the calling agent or open
/// (no target). `"force": true` in the payload widens the pool to capsules
/// targeting other agents; such an override is recorded on the capsule as
/// `consumed_despite_target`. A capsule created by the calling session is
/// never claimed back (self-reconsume guard). Empty stdout when nothing
/// matches — capsules for other agents are then listed in diagnostics so
/// they are never silently hidden.
fn handle_handoff_consume(req: &ai_handoff_ipc::protocol::Request) -> Response {
    let raw = raw_with_request_fallbacks(req);
    let cwd = raw
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(&req.cwd));
    let project_id = fingerprint(&cwd);
    let Some(agent) = canonical_agent_id(&req.agent) else {
        return degraded(&req.request_id, "daemon_error");
    };
    let force = raw.get("force").and_then(Value::as_bool).unwrap_or(false);

    // Explicit claim: `--id <capsule>` names exactly one capsule and consumes
    // it regardless of target (naming it IS the consent) — the safe path when
    // several capsules are pending and a blind newest-first pick could grab
    // the wrong one.
    let capsule = if let Some(capsule_id) = raw.get("capsule_id").and_then(Value::as_str) {
        match list_pending(&project_id)
            .into_iter()
            .find(|capsule| capsule.capsule_id == capsule_id)
        {
            Some(capsule) => Some(capsule),
            None => return degraded(&req.request_id, "capsule_not_found"),
        }
    } else {
        let candidates = if force {
            list_pending(&project_id)
        } else {
            pending_for(&project_id, &agent)
        };
        candidates
            .into_iter()
            .find(|capsule| !same_session(capsule, &req.session_id))
    };

    match capsule {
        Some(capsule) => {
            let despite_target = capsule
                .target_agent
                .as_deref()
                .is_some_and(|target| target != agent);
            let context = render_capsule_context_for_cwd(&capsule, &cwd);
            if mark_consumed(
                &project_id,
                &capsule.capsule_id,
                &agent,
                Utc::now(),
                despite_target,
            )
            .is_err()
            {
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
                    "consumed_despite_target": despite_target,
                }),
                json!({}),
            )
        }
        None => {
            let others = other_pending_summaries(&project_id, &agent);
            // Mismatched capsules must not vanish behind a bare {} — say in
            // stdout that they exist. Only a truly empty store keeps the
            // legacy {} shape skills already understand.
            let stdout = if others.is_empty() {
                json!({})
            } else {
                json!({ "pending": false, "others": others })
            };
            Router::ok(req, stdout, json!({ "pending": false }))
        }
    }
}

/// Read-only preview of the pending capsule (`ai-handoff handoff --peek`).
/// Returns the same rendered context a consume would inject, but never marks
/// the capsule consumed — so the user can inspect what would enter the
/// context before deciding to run the real handoff. Pending capsules that
/// target a different agent are reported under `others` (id, target, goal)
/// so a wrong target can be noticed and fixed with retarget or --force.
fn handle_handoff_peek(req: &ai_handoff_ipc::protocol::Request) -> Response {
    let raw = raw_with_request_fallbacks(req);
    let cwd = raw
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(&req.cwd));
    let project_id = fingerprint(&cwd);
    let Some(agent) = canonical_agent_id(&req.agent) else {
        return degraded(&req.request_id, "daemon_error");
    };

    let others = other_pending_summaries(&project_id, &agent);
    match find_pending(&project_id, &agent) {
        Some(capsule) => Router::ok(
            req,
            json!({
                "pending": true,
                "capsule_id": capsule.capsule_id,
                "created_at": capsule.created_at,
                "preview": render_capsule_context_for_cwd(&capsule, &cwd),
                "others": others,
            }),
            json!({}),
        ),
        None => Router::ok(
            req,
            json!({ "pending": false, "others": others }),
            json!({}),
        ),
    }
}

/// Re-point a pending capsule at another agent, or open it up
/// (`ai-handoff retarget <capsule-id> [--to <agent>]`). The explicit fix-up
/// path for a capsule saved with the wrong target.
fn handle_handoff_retarget(req: &ai_handoff_ipc::protocol::Request) -> Response {
    let raw = raw_with_request_fallbacks(req);
    let cwd = raw
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(&req.cwd));
    let project_id = fingerprint(&cwd);
    let Some(capsule_id) = raw.get("capsule_id").and_then(Value::as_str) else {
        return degraded(&req.request_id, "daemon_error");
    };
    let target = capsule_target(&raw);

    match crate::store::retarget(&project_id, capsule_id, target.clone()) {
        Ok(()) => Router::ok(
            req,
            json!({
                "retargeted": true,
                "capsule_id": capsule_id,
                "target_agent": target,
            }),
            json!({}),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            degraded(&req.request_id, "capsule_not_found")
        }
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            degraded(&req.request_id, "capsule_not_pending")
        }
        Err(_) => degraded(&req.request_id, "daemon_error"),
    }
}

/// True when the capsule was created by the session now asking to consume it —
/// same-agent resume is allowed, but a session never re-eats its own capsule.
fn same_session(capsule: &Capsule, session_id: &Option<String>) -> bool {
    matches!(
        (capsule.session.session_id.as_deref(), session_id.as_deref()),
        (Some(own), Some(caller)) if own == caller
    )
}

/// Short summaries of pending capsules addressed to OTHER agents — surfaced in
/// peek/consume responses so misrouted capsules stay visible.
fn other_pending_summaries(project_id: &str, agent: &str) -> Vec<Value> {
    list_pending(project_id)
        .into_iter()
        .filter(|capsule| {
            capsule
                .target_agent
                .as_deref()
                .is_some_and(|target| target != agent)
        })
        .map(|capsule| {
            json!({
                "capsule_id": capsule.capsule_id,
                "target_agent": capsule.target_agent,
                "created_at": capsule.created_at,
                "goal": capsule.summary.goal,
            })
        })
        .collect()
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

/// Cap on file paths rendered into handoff context — the capsule may carry
/// more, but the injected context must not balloon with long change lists.
const RENDER_FILES_MAX: usize = 10;

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
    if !capsule.summary.risks.is_empty() {
        lines.push(format!("risks: {}", capsule.summary.risks.join("; ")));
    }
    if !capsule.files.is_empty() {
        let shown = capsule
            .files
            .iter()
            .take(RENDER_FILES_MAX)
            .map(|file| match &file.status {
                Some(status) => format!("{} ({status})", file.path),
                None => file.path.clone(),
            })
            .collect::<Vec<_>>()
            .join("; ");
        let overflow = capsule.files.len().saturating_sub(RENDER_FILES_MAX);
        if overflow > 0 {
            lines.push(format!("files: {shown} (+{overflow} more)"));
        } else {
            lines.push(format!("files: {shown}"));
        }
    }
    if let Some(next) = &capsule.next_prompt {
        lines.push(format!("next_prompt: {next}"));
    }
    if let Some(ws) = &capsule.workspace {
        let mut parts = Vec::new();
        if let Some(branch) = &ws.branch {
            parts.push(format!("branch {branch}"));
        }
        if let Some(sha) = &ws.head_sha {
            parts.push(format!("HEAD {}", short_sha(sha)));
        }
        if let Some(dirty) = ws.dirty_files {
            parts.push(format!("{dirty} dirty file(s)"));
        }
        if !parts.is_empty() {
            lines.push(format!("workspace: {}", parts.join(", ")));
        }
    }
    lines.join("\n")
}

fn render_capsule_context_for_cwd(capsule: &Capsule, cwd: &std::path::Path) -> String {
    let mut context = render_capsule_context(capsule);
    if let Some(note) = workspace_drift_note(capsule, cwd) {
        context.push('\n');
        context.push_str(&note);
    }
    context
}

/// The git snapshot for a capsule (best-effort; `None` outside a repo).
fn workspace_snapshot(cwd: &std::path::Path) -> Option<ai_handoff_core::capsule::Workspace> {
    let snap = ai_handoff_core::git_info::collect(cwd)?;
    Some(ai_handoff_core::capsule::Workspace {
        branch: snap.branch,
        head_sha: snap.head_sha,
        dirty_files: snap.dirty_files,
    })
}

fn short_sha(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}

/// A note appended to consumed-capsule context when the workspace moved since
/// the capsule was created (resume-time drift detection).
fn workspace_drift_note(capsule: &Capsule, cwd: &std::path::Path) -> Option<String> {
    let capsule_sha = capsule.workspace.as_ref()?.head_sha.as_deref()?;
    let current_sha = ai_handoff_core::git_info::head_sha(cwd)?;
    if current_sha == capsule_sha {
        return None;
    }
    Some(format!(
        "[workspace drift] This capsule was created at commit {} but the workspace is now at {} — verify the capsule still matches the checkout before acting on it.",
        short_sha(capsule_sha),
        short_sha(&current_sha),
    ))
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
    cwd: &std::path::Path,
    session_id: Option<String>,
    source_agent: &str,
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
        source_agent: source_agent.to_string(),
        // Routing hint only, and only when the payload asks for one — the
        // default is an open capsule any agent may pick up. No more
        // "opposite agent" guessing: with 3+ agents there is no opposite.
        target_agent: capsule_target(payload),
        session: Session {
            session_id,
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
        // Collected by the daemon itself, never taken from the agent payload,
        // so the consuming side can trust it for drift detection.
        workspace: workspace_snapshot(cwd),
        redaction: RedactionMeta {
            applied: redacted,
            ruleset: "default-v2".to_string(),
        },
        consumption: Consumption {
            state: ConsumptionState::Pending,
            consumed_by: None,
            consumed_at: None,
            consumed_despite_target: false,
        },
    }
}

/// The requested target from a checkpoint/retarget payload: `target` (or
/// legacy `target_agent`) as a canonical agent id. Absent, null, or the
/// explicit "none"/"any"/"open" spellings mean an open capsule.
fn capsule_target(payload: &Value) -> Option<String> {
    let value = payload
        .get("target")
        .or_else(|| payload.get("target_agent"))?
        .as_str()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "none" | "any" | "open" => None,
        _ => canonical_agent_id(value),
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
mod provider_fallback_tests {
    use super::*;

    fn usage(used: f64) -> TriggerUsage {
        TriggerUsage {
            used_percent: used,
            resets_at_ms: None,
        }
    }

    #[test]
    fn consult_provider_only_when_local_cannot_confirm() {
        // Off mode never consults.
        assert!(!should_consult_provider(TriggerMode::Off, None, 10.0));
        // No local sample → consult.
        assert!(should_consult_provider(TriggerMode::Ask, None, 10.0));
        // Local below threshold → consult (it might be a stale/other-account low).
        assert!(should_consult_provider(TriggerMode::Ask, Some(5.0), 10.0));
        // Local already over threshold → fire without a network call.
        assert!(!should_consult_provider(
            TriggerMode::Auto,
            Some(40.0),
            10.0
        ));
        assert!(!should_consult_provider(TriggerMode::Ask, Some(10.0), 10.0));
    }

    #[test]
    fn pick_higher_usage_prefers_larger_and_falls_back_to_provider() {
        // No local → provider.
        assert_eq!(pick_higher_usage(None, usage(46.0)).used_percent, 46.0);
        // Provider higher → provider (real overage the stale local missed).
        assert_eq!(
            pick_higher_usage(Some(usage(3.0)), usage(46.0)).used_percent,
            46.0
        );
        // Local higher → keep local (never regress a known-higher reading).
        assert_eq!(
            pick_higher_usage(Some(usage(80.0)), usage(46.0)).used_percent,
            80.0
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use ai_handoff_core::{
        capsule::{Capsule, Consumption, ConsumptionState, RedactionMeta, Session, Summary},
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
            source_agent: "codex".into(),
            target_agent: Some("claude-code".into()),
            session: Session::default(),
            summary: Summary {
                goal: "continue router".into(),
                done: vec!["core".into()],
                remaining: vec!["ipc".into()],
                risks: vec!["ipc schema drift".into()],
            },
            files: vec![
                FileChange {
                    path: "src/router.rs".into(),
                    status: Some("modified".into()),
                    summary: None,
                },
                FileChange {
                    path: "src/store.rs".into(),
                    status: None,
                    summary: None,
                },
            ],
            next_prompt: Some("pick up".into()),
            workspace: None,
            redaction: RedactionMeta {
                applied: true,
                ruleset: "default-v2".into(),
            },
            consumption: Consumption {
                state: ConsumptionState::Pending,
                consumed_by: None,
                consumed_at: None,
                consumed_despite_target: false,
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
        let pending = crate::store::find_pending(&project_id, "claude-code").unwrap();
        assert_eq!(pending.summary.goal, "ship MVP");
        assert_eq!(pending.source_agent, "codex");
        // No target in the payload → open capsule, not "the opposite agent".
        assert_eq!(pending.target_agent, None);
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    fn run_git(dir: &std::path::Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn stop_capsule_attaches_git_workspace_metadata() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        // Skip silently when git is unavailable (the daemon behaves the same:
        // capsules simply carry no workspace block).
        if !run_git(cwd.path(), &["init", "-b", "main"]) {
            return;
        }
        assert!(run_git(
            cwd.path(),
            &["config", "user.email", "t@example.com"]
        ));
        assert!(run_git(cwd.path(), &["config", "user.name", "t"]));
        std::fs::write(cwd.path().join("a.txt"), b"one").unwrap();
        assert!(run_git(cwd.path(), &["add", "."]));
        assert!(run_git(
            cwd.path(),
            &["commit", "--no-gpg-sign", "-m", "init"]
        ));
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let router = Router::new();
        let req = request(
            "turn-git",
            "stop",
            "codex",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "last_assistant_message": "```ai-handoff-capsule\n{\"goal\":\"git meta\"}\n```"
            }),
        );
        assert_eq!(router.handle(&req).status, Status::Ok);

        let project_id = fingerprint(cwd.path());
        let pending = crate::store::find_pending(&project_id, "claude-code").unwrap();
        let ws = pending.workspace.expect("workspace snapshot in a git repo");
        assert_eq!(ws.branch.as_deref(), Some("main"));
        assert_eq!(ws.head_sha.map(|sha| sha.len()), Some(40));
        assert_eq!(ws.dirty_files, Some(0));
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_consume_appends_workspace_drift_note() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        if !run_git(cwd.path(), &["init", "-b", "main"]) {
            return;
        }
        assert!(run_git(
            cwd.path(),
            &["config", "user.email", "t@example.com"]
        ));
        assert!(run_git(cwd.path(), &["config", "user.name", "t"]));
        std::fs::write(cwd.path().join("a.txt"), b"one").unwrap();
        assert!(run_git(cwd.path(), &["add", "."]));
        assert!(run_git(
            cwd.path(),
            &["commit", "--no-gpg-sign", "-m", "init"]
        ));
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        // Pending capsule recorded at a different (fake) commit.
        let project_id = fingerprint(cwd.path());
        let mut capsule = pending_capsule(&project_id);
        capsule.workspace = Some(ai_handoff_core::capsule::Workspace {
            branch: Some("main".into()),
            head_sha: Some("0123456789abcdef0123456789abcdef01234567".into()),
            dirty_files: Some(0),
        });
        crate::store::save_capsule(&capsule).unwrap();

        let router = Router::new();
        let mut req = request(
            "turn-drift",
            "handoff",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        req.kind = "handoff_consume".into();
        let resp = router.handle(&req);
        let context = resp.hook_stdout["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(context.contains("[workspace drift]"), "got: {context}");
        assert!(context.contains("0123456789"));
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_peek_appends_workspace_drift_note_without_consuming() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        if !run_git(cwd.path(), &["init", "-b", "main"]) {
            return;
        }
        assert!(run_git(
            cwd.path(),
            &["config", "user.email", "t@example.com"]
        ));
        assert!(run_git(cwd.path(), &["config", "user.name", "t"]));
        std::fs::write(cwd.path().join("a.txt"), b"one").unwrap();
        assert!(run_git(cwd.path(), &["add", "."]));
        assert!(run_git(
            cwd.path(),
            &["commit", "--no-gpg-sign", "-m", "init"]
        ));
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let project_id = fingerprint(cwd.path());
        let mut capsule = pending_capsule(&project_id);
        capsule.workspace = Some(ai_handoff_core::capsule::Workspace {
            branch: Some("main".into()),
            head_sha: Some("fedcba9876543210fedcba9876543210fedcba98".into()),
            dirty_files: Some(0),
        });
        crate::store::save_capsule(&capsule).unwrap();

        let router = Router::new();
        let mut req = request(
            "turn-drift-peek",
            "handoff",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        req.kind = "handoff_peek".into();
        let resp = router.handle(&req);
        let preview = resp.hook_stdout["preview"].as_str().unwrap();
        assert!(preview.contains("[workspace drift]"), "got: {preview}");
        assert!(preview.contains("fedcba9876"));
        assert!(crate::store::find_pending(&project_id, "claude-code").is_some());
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
        let pending = crate::store::find_pending(&project_id, "claude-code").unwrap();
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
        assert!(crate::store::find_pending(&project_id, "claude-code").is_some());

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
        let context = resp.hook_stdout["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(context.contains("continue router"));
        assert!(context.contains("risks: ipc schema drift"));
        assert!(context.contains("files: src/router.rs (modified); src/store.rs"));
        assert_eq!(resp.hook_stdout["consumed"], true);
        assert!(crate::store::find_pending(&project_id, "claude-code").is_none());
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn render_capsule_context_caps_file_list() {
        let project_id = "projX";
        let mut capsule = pending_capsule(project_id);
        capsule.files = (0..15)
            .map(|idx| FileChange {
                path: format!("src/file_{idx}.rs"),
                status: None,
                summary: None,
            })
            .collect();

        let context = render_capsule_context(&capsule);
        // Only the first RENDER_FILES_MAX paths appear, with an overflow note.
        assert!(context.contains("src/file_0.rs"));
        assert!(context.contains("src/file_9.rs"));
        assert!(!context.contains("src/file_10.rs"));
        assert!(context.contains("(+5 more)"));
    }

    #[test]
    fn handoff_peek_previews_without_consuming() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());
        crate::store::save_capsule(&pending_capsule(&project_id)).unwrap();

        let router = Router::new();
        let mut req = request(
            "turn-peek",
            "handoff",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        req.kind = "handoff_peek".into();
        let resp = router.handle(&req);
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(resp.hook_stdout["pending"], true);
        assert_eq!(resp.hook_stdout["capsule_id"], "cap_20260625_120000_abcd");
        assert!(resp.hook_stdout["preview"]
            .as_str()
            .unwrap()
            .contains("continue router"));
        // Peek never consumes: the capsule must still be pending.
        assert!(crate::store::find_pending(&project_id, "claude-code").is_some());

        // Wrong agent sees nothing.
        let mut wrong = request(
            "turn-peek-wrong",
            "handoff",
            "codex",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        wrong.kind = "handoff_peek".into();
        let resp2 = router.handle(&wrong);
        assert_eq!(resp2.hook_stdout["pending"], false);
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
        // Not silently hidden: when a mismatched capsule exists, stdout says
        // so — a bare {} would read as "nothing pending at all".
        assert_eq!(resp.hook_stdout["pending"], false);
        assert_eq!(
            resp.hook_stdout["others"][0]["capsule_id"],
            "cap_20260625_120000_abcd"
        );
        assert_eq!(resp.hook_stdout["others"][0]["target_agent"], "claude-code");
        assert!(crate::store::find_pending(&project_id, "claude-code").is_some());
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_consume_with_nothing_pending_keeps_empty_stdout() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let router = Router::new();
        let mut req = request(
            "turn-consume-empty",
            "handoff",
            "codex",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        req.kind = "handoff_consume".into();
        let resp = router.handle(&req);
        // No capsules at all → {} stays, preserving the skill contract.
        assert_eq!(resp.hook_stdout, json!({}));
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_consume_by_capsule_id_claims_that_capsule() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());
        // Older capsule locked to claude-code + newer open capsule. A blind
        // force would eat the newest; --id must claim exactly the named one.
        crate::store::save_capsule(&pending_capsule(&project_id)).unwrap();
        let mut open = pending_capsule(&project_id);
        open.capsule_id = "cap_20260625_130000_ffff".into();
        open.created_at = "2026-06-25T13:00:00Z".into();
        open.target_agent = None;
        crate::store::save_capsule(&open).unwrap();

        let router = Router::new();
        let mut req = request(
            "turn-claim",
            "handoff",
            "grok",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "capsule_id": "cap_20260625_120000_abcd",
            }),
        );
        req.kind = "handoff_consume".into();
        let resp = router.handle(&req);
        assert_eq!(resp.hook_stdout["consumed"], true);
        assert_eq!(resp.hook_stdout["capsule_id"], "cap_20260625_120000_abcd");
        // Explicit claim of a claude-code capsule by grok is an override.
        assert_eq!(resp.hook_stdout["consumed_despite_target"], true);
        // The open capsule is untouched.
        assert_eq!(
            crate::store::find_pending(&project_id, "codex")
                .unwrap()
                .capsule_id,
            "cap_20260625_130000_ffff"
        );
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_consume_by_unknown_capsule_id_degrades() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let router = Router::new();
        let mut req = request(
            "turn-claim-missing",
            "handoff",
            "grok",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "capsule_id": "cap_nope",
            }),
        );
        req.kind = "handoff_consume".into();
        let resp = router.handle(&req);
        assert_eq!(resp.status, Status::Degraded);
        assert_eq!(resp.warnings, vec!["capsule_not_found"]);
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_consume_open_capsule_by_unknown_agent() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());
        let mut capsule = pending_capsule(&project_id);
        capsule.target_agent = None;
        crate::store::save_capsule(&capsule).unwrap();

        // Grok is not in any enum — an open capsule must still reach it.
        let router = Router::new();
        let mut req = request(
            "turn-grok",
            "handoff",
            "grok",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        req.kind = "handoff_consume".into();
        let resp = router.handle(&req);
        assert_eq!(resp.hook_stdout["consumed"], true);
        assert_eq!(resp.hook_stdout["consumed_despite_target"], false);
        assert!(crate::store::find_pending(&project_id, "grok").is_none());
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_consume_force_takes_other_target_and_records_it() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());
        // Capsule locked to claude-code; grok takes it only with force.
        crate::store::save_capsule(&pending_capsule(&project_id)).unwrap();

        let router = Router::new();
        let mut req = request(
            "turn-force",
            "handoff",
            "grok",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy(), "force": true }),
        );
        req.kind = "handoff_consume".into();
        let resp = router.handle(&req);
        assert_eq!(resp.hook_stdout["consumed"], true);
        assert_eq!(resp.hook_stdout["consumed_despite_target"], true);
        assert!(crate::store::find_pending(&project_id, "claude-code").is_none());
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_consume_skips_capsule_created_by_same_session() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());
        let mut capsule = pending_capsule(&project_id);
        capsule.session.session_id = Some("s1".into());
        crate::store::save_capsule(&capsule).unwrap();

        let router = Router::new();
        // request() uses session_id "s1" — the session that wrote the capsule.
        let mut own = request(
            "turn-own",
            "handoff",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        own.kind = "handoff_consume".into();
        let resp = router.handle(&own);
        assert_eq!(resp.hook_stdout, json!({}));
        assert!(crate::store::find_pending(&project_id, "claude-code").is_some());

        // A different session of the SAME agent may resume it.
        let mut other = request(
            "turn-other",
            "handoff",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        other.kind = "handoff_consume".into();
        other.session_id = Some("s2".into());
        let resp2 = router.handle(&other);
        assert_eq!(resp2.hook_stdout["consumed"], true);
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_peek_lists_other_target_capsules() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());
        crate::store::save_capsule(&pending_capsule(&project_id)).unwrap();

        let router = Router::new();
        let mut req = request(
            "turn-peek-others",
            "handoff",
            "grok",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy() }),
        );
        req.kind = "handoff_peek".into();
        let resp = router.handle(&req);
        assert_eq!(resp.hook_stdout["pending"], false);
        assert_eq!(
            resp.hook_stdout["others"][0]["capsule_id"],
            "cap_20260625_120000_abcd"
        );
        assert_eq!(resp.hook_stdout["others"][0]["target_agent"], "claude-code");
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn handoff_retarget_moves_capsule_to_new_agent() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());
        crate::store::save_capsule(&pending_capsule(&project_id)).unwrap();

        let router = Router::new();
        let mut req = request(
            "turn-retarget",
            "retarget",
            "grok",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "capsule_id": "cap_20260625_120000_abcd",
                "target": "grok",
            }),
        );
        req.kind = "handoff_retarget".into();
        let resp = router.handle(&req);
        assert_eq!(resp.hook_stdout["retargeted"], true);
        assert!(crate::store::find_pending(&project_id, "grok").is_some());
        assert!(crate::store::find_pending(&project_id, "claude-code").is_none());

        // "none" opens the capsule for everyone.
        let mut open = request(
            "turn-retarget-open",
            "retarget",
            "grok",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "capsule_id": "cap_20260625_120000_abcd",
                "target": "none",
            }),
        );
        open.kind = "handoff_retarget".into();
        assert_eq!(router.handle(&open).hook_stdout["retargeted"], true);
        assert!(crate::store::find_pending(&project_id, "claude-code").is_some());

        // Unknown capsule id degrades with a specific warning.
        let mut missing = request(
            "turn-retarget-missing",
            "retarget",
            "grok",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "capsule_id": "cap_nope",
            }),
        );
        missing.kind = "handoff_retarget".into();
        let resp = router.handle(&missing);
        assert_eq!(resp.status, Status::Degraded);
        assert_eq!(resp.warnings, vec!["capsule_not_found"]);
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn checkpoint_uses_calling_agent_and_optional_target() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let project_id = fingerprint(cwd.path());

        let router = Router::new();
        let mut req = request(
            "turn-ckpt",
            "checkpoint",
            "grok",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "goal": "handoff from grok",
                "target": "gemini",
            }),
        );
        req.kind = "checkpoint".into();
        let resp = router.handle(&req);
        assert_eq!(resp.hook_stdout["saved"], true);

        let pending = crate::store::find_pending(&project_id, "gemini").unwrap();
        assert_eq!(pending.source_agent, "grok");
        assert_eq!(pending.target_agent.as_deref(), Some("gemini"));
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
    fn post_tool_use_claude_fires_from_raw_rate_limits_without_statusline_sample() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[triggers.five_hour]\nenabled = true\nthreshold_percent = 10\nmode = \"ask\"\n[capsule]\nlanguage = \"en\"\n",
        )
        .unwrap();

        let router = Router::new();
        let req = request(
            "turn-claude-raw",
            "post-tool-use",
            "claude-code",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "session_id": "s-claude-raw",
                "rate_limits": {
                    "five_hour": { "used_percentage": 78.0, "resets_at": 4102444800.0 }
                }
            }),
        );
        let resp = router.handle(&req);
        assert_eq!(
            resp.diagnostics["trigger_fired"], true,
            "{:?}",
            resp.diagnostics
        );
        assert_eq!(resp.diagnostics["used_percent"], 78.0);
        assert_eq!(resp.diagnostics["usage_source"], "raw-rate-limits");
        assert_eq!(resp.hook_stdout["decision"], "block");
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
    fn post_tool_use_codex_fires_from_session_rollout_without_transcript_path() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let codex_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CODEX_HOME", codex_home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[triggers.five_hour]\nenabled = true\nthreshold_percent = 10\nmode = \"ask\"\n[capsule]\nlanguage = \"ko\"\n",
        )
        .unwrap();

        let sid = "0197e5c3-1111-2222-3333-444455556666";
        let day_dir = codex_home.path().join("sessions/2026/07/05");
        std::fs::create_dir_all(&day_dir).unwrap();
        std::fs::write(
            day_dir.join(format!("rollout-2026-07-05T03-00-00-{sid}.jsonl")),
            "{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":29.0,\"resets_at\":4102444800}}}}\n",
        )
        .unwrap();

        // No transcript_path in the hook input — only cwd + session_id.
        let router = Router::new();
        let req = request(
            "turn-codex-rollout",
            "post-tool-use",
            "codex",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy(), "session_id": sid }),
        );
        let resp = router.handle(&req);
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(
            resp.diagnostics["trigger_fired"], true,
            "{:?}",
            resp.diagnostics
        );
        assert_eq!(resp.diagnostics["used_percent"], 29.0);
        assert_eq!(resp.diagnostics["usage_source"], "session-rollout");
        assert_eq!(resp.hook_stdout["decision"], "block");
        assert!(resp.hook_stdout["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("네"));

        std::env::remove_var("CODEX_HOME");
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn post_tool_use_codex_fires_from_latest_rollout_without_session_match() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let codex_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CODEX_HOME", codex_home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[triggers.five_hour]\nenabled = true\nthreshold_percent = 10\nmode = \"ask\"\n[capsule]\nlanguage = \"en\"\n",
        )
        .unwrap();

        let day_dir = codex_home.path().join("sessions/2026/07/05");
        std::fs::create_dir_all(&day_dir).unwrap();
        let latest = day_dir.join("rollout-2026-07-05T03-00-00-current-session.jsonl");
        std::fs::write(
            &latest,
            "{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":78.0,\"resets_at\":4102444800}}}}\n",
        )
        .unwrap();

        let router = Router::new();
        let req = request(
            "turn-codex-latest-rollout",
            "post-tool-use",
            "codex",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy(), "session_id": "not-in-rollout-name" }),
        );
        let resp = router.handle(&req);
        assert_eq!(
            resp.diagnostics["trigger_fired"], true,
            "{:?}",
            resp.diagnostics
        );
        assert_eq!(resp.diagnostics["used_percent"], 78.0);
        assert_eq!(resp.diagnostics["usage_source"], "latest-rollout");
        assert_eq!(resp.hook_stdout["decision"], "block");

        std::env::remove_var("CODEX_HOME");
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn post_tool_use_codex_fires_from_raw_rate_limits() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let codex_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CODEX_HOME", codex_home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[triggers.five_hour]\nenabled = true\nthreshold_percent = 10\nmode = \"ask\"\n",
        )
        .unwrap();

        let router = Router::new();
        let req = request(
            "turn-codex-raw",
            "post-tool-use",
            "codex",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "session_id": "s-raw",
                "payload": { "rate_limits": { "primary": { "used_percent": 29.0 } } }
            }),
        );
        let resp = router.handle(&req);
        assert_eq!(
            resp.diagnostics["trigger_fired"], true,
            "{:?}",
            resp.diagnostics
        );
        assert_eq!(resp.diagnostics["usage_source"], "raw-rate-limits");
        assert_eq!(resp.hook_stdout["decision"], "block");
        std::env::remove_var("CODEX_HOME");
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn post_tool_use_codex_reports_unknown_reasons_when_nothing_found() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let codex_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CODEX_HOME", codex_home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[triggers.five_hour]\nenabled = true\nthreshold_percent = 10\nmode = \"ask\"\n",
        )
        .unwrap();

        let router = Router::new();
        let req = request(
            "turn-codex-unknown",
            "post-tool-use",
            "codex",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy(), "session_id": "s-none" }),
        );
        let resp = router.handle(&req);
        assert_eq!(resp.diagnostics["trigger_fired"], false);
        assert_eq!(resp.diagnostics["trigger_reason"], "unknown");
        assert_eq!(
            resp.diagnostics["usage_unknown_reasons"],
            json!([
                "no-raw-rate-limits",
                "no-transcript-path",
                "session-rollout-not-found",
                "latest-rollout-not-found"
            ])
        );
        std::env::remove_var("CODEX_HOME");
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn stop_fires_claude_trigger_for_read_only_turns() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::fs::write(
            home.path().join("config.toml"),
            "[triggers.five_hour]\nenabled = true\nthreshold_percent = 10\nmode = \"ask\"\n[capsule]\nlanguage = \"ko\"\n",
        )
        .unwrap();
        let now_ms = Utc::now().timestamp_millis();
        assert!(ai_handoff_core::sensor::record_claude_rate_limit(
            &json!({
                "session_id": "sid-stop-trigger",
                "rate_limits": { "five_hour": { "used_percentage": 29.0 } }
            }),
            now_ms,
        ));

        // A read-only turn never runs the PostToolUse hook (matcher is
        // Write|Edit|Bash) — Stop must still evaluate the threshold.
        let router = Router::new();
        let req = request(
            "turn-stop-trigger",
            "stop",
            "claude-code",
            cwd.path(),
            json!({ "cwd": cwd.path().to_string_lossy(), "session_id": "s-stop" }),
        );
        let resp = router.handle(&req);
        assert_eq!(
            resp.diagnostics["trigger_fired"], true,
            "{:?}",
            resp.diagnostics
        );
        assert_eq!(resp.hook_stdout["decision"], "block");
        // Stop hooks have no additionalContext channel; the full ask context
        // rides in the block reason instead.
        let reason = resp.hook_stdout["reason"].as_str().unwrap();
        assert!(reason.contains("AskUserQuestion"));
        assert!(reason.contains("ai-handoff checkpoint --agent claude-code"));
        assert!(resp.hook_stdout.get("hookSpecificOutput").is_none());

        // A Stop that ships a capsule skips the trigger entirely.
        let with_capsule = request(
            "turn-stop-capsule",
            "stop",
            "claude-code",
            cwd.path(),
            json!({
                "cwd": cwd.path().to_string_lossy(),
                "session_id": "s-stop-2",
                "last_assistant_message": "```ai-handoff-capsule\n{\"goal\":\"done\"}\n```"
            }),
        );
        let resp2 = router.handle(&with_capsule);
        assert_eq!(resp2.hook_stdout, json!({}));
        assert_eq!(resp2.diagnostics["capsule_saved"], true);
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
