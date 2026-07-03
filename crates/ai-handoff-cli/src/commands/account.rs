//! `ai-handoff account list|status|doctor` — read-only account inspection.
//!
//! The mutating actions (add / switch / launch / delete) live in the TUI Account
//! tab, since they touch credentials and should require an explicit local action.

use ai_handoff_core::account::{self, AccountSlot, AccountStatus, Agent, Identity};

use crate::AccountAction;

pub fn run(action: AccountAction) -> anyhow::Result<i32> {
    match action {
        AccountAction::List { json } => list(json),
        AccountAction::Status { json, fetch } => status(json, fetch),
        AccountAction::Doctor { json } => doctor(json),
    }
}

const AGENTS: [(Agent, &str); 2] = [(Agent::Codex, "codex"), (Agent::Claude, "claude")];

fn list(json: bool) -> anyhow::Result<i32> {
    if json {
        let accounts: Vec<_> = AGENTS
            .iter()
            .flat_map(|&(agent, name)| {
                account::list_slots(agent).into_iter().map(move |s| {
                    serde_json::json!({
                        "agent": name,
                        "label": s.meta.label,
                        "email": s.meta.email,
                        "plan_hint": s.meta.plan_hint,
                        "active": s.active,
                        "source": s.meta.source,
                        "created_at": s.meta.created_at,
                    })
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "accounts": accounts }))?
        );
        return Ok(0);
    }
    for (agent, name) in AGENTS {
        println!("{}:", title(name));
        let slots = account::list_slots(agent);
        if slots.is_empty() {
            println!("  (none — add one in the TUI Account tab)");
        }
        for s in slots {
            let mark = if s.active { "●" } else { "○" };
            println!(
                "  {mark} {:<26} {:<26} {}",
                s.meta.label,
                s.meta.email.as_deref().unwrap_or("-"),
                s.meta.plan_hint.as_deref().unwrap_or("-"),
            );
        }
    }
    Ok(0)
}

fn status(json: bool, fetch: bool) -> anyhow::Result<i32> {
    let codex = (account::codex_identity(), account::codex_status());
    let claude_id = account::claude_identity();
    let claude_status = if fetch {
        claude_fetched_status(claude_id.as_ref()).or_else(account::claude_status)
    } else {
        account::claude_status()
    };
    let claude = (claude_id, claude_status);
    if json {
        let payload = serde_json::json!({
            "codex": status_json(codex.0.as_ref(), codex.1.as_ref()),
            "claude": status_json(claude.0.as_ref(), claude.1.as_ref()),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(0);
    }
    print_status("Codex", codex.0.as_ref(), codex.1.as_ref());
    print_status("Claude", claude.0.as_ref(), claude.1.as_ref());
    Ok(0)
}

fn claude_fetched_status(id: Option<&Identity>) -> Option<AccountStatus> {
    let slots = account::list_slots(Agent::Claude);
    let selected = select_status_slot(&slots, id)?;
    ai_handoff_tui::account_api::fetch_slot_usage(Agent::Claude, &selected.meta.label)
        .ok()
        .map(account_status_from_usage)
}

fn select_status_slot<'a>(
    slots: &'a [AccountSlot],
    live: Option<&Identity>,
) -> Option<&'a AccountSlot> {
    slots.iter().find(|s| s.active).or_else(|| {
        let email = live?.email.as_deref()?;
        slots.iter().find(|s| {
            s.meta
                .email
                .as_deref()
                .is_some_and(|e| e.eq_ignore_ascii_case(email))
                || s.meta.label.eq_ignore_ascii_case(email)
        })
    })
}

fn doctor(json: bool) -> anyhow::Result<i32> {
    let codex_in = account::codex_identity().is_some();
    let claude_in = account::claude_identity().is_some();
    let codex_slots = account::list_slots(Agent::Codex).len();
    let claude_slots = account::list_slots(Agent::Claude).len();
    let codex_cli = account::which("codex").is_some();
    let claude_cli = account::which("claude").is_some();
    let codex_running = account::agent_running(Agent::Codex);
    let claude_running = account::agent_running(Agent::Claude);

    let mut warnings: Vec<String> = Vec::new();
    if !codex_cli {
        warnings
            .push("`codex` CLI not on PATH — adding/launching Codex accounts won't work".into());
    }
    if !claude_cli {
        warnings
            .push("`claude` CLI not on PATH — adding/launching Claude accounts won't work".into());
    }
    if !codex_in && codex_slots == 0 {
        warnings.push("No Codex account signed in or saved".into());
    }
    if !claude_in && claude_slots == 0 {
        warnings.push("No Claude account signed in or saved".into());
    }

    if json {
        let payload = serde_json::json!({
            "codex": { "signed_in": codex_in, "saved_slots": codex_slots, "cli_on_path": codex_cli, "running": codex_running },
            "claude": { "signed_in": claude_in, "saved_slots": claude_slots, "cli_on_path": claude_cli, "running": claude_running },
            "warnings": warnings,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(if warnings.is_empty() { 0 } else { 1 });
    }

    println!(
        "Codex:  signed in: {}   saved: {codex_slots}   cli: {}   running: {}",
        yn(codex_in),
        yn(codex_cli),
        yn(codex_running)
    );
    println!(
        "Claude: signed in: {}   saved: {claude_slots}   cli: {}   running: {}",
        yn(claude_in),
        yn(claude_cli),
        yn(claude_running)
    );
    if warnings.is_empty() {
        println!("\nOK — no problems found.");
        Ok(0)
    } else {
        println!("\nWarnings:");
        for w in &warnings {
            println!("  - {w}");
        }
        Ok(1)
    }
}

// --- helpers -------------------------------------------------------------

fn title(name: &str) -> &str {
    match name {
        "codex" => "Codex",
        "claude" => "Claude",
        other => other,
    }
}

fn yn(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn plan(id: Option<&Identity>, st: Option<&AccountStatus>) -> String {
    st.and_then(|s| s.plan_type.clone())
        .or_else(|| id.and_then(|i| i.plan_type.clone()))
        .unwrap_or_else(|| "unknown".into())
}

fn print_status(name: &str, id: Option<&Identity>, st: Option<&AccountStatus>) {
    let email = id
        .and_then(|i| i.email.as_deref())
        .unwrap_or("(not signed in)");
    println!("{name}: {email}   plan: {}", plan(id, st));
    if let Some(w) = st.and_then(|s| s.five_hour.as_ref()) {
        println!(
            "  5h:     {:>3.0}% used{}",
            w.used_percent,
            reset(w.resets_at)
        );
    }
    if let Some(w) = st.and_then(|s| s.weekly.as_ref()) {
        println!(
            "  weekly: {:>3.0}% used{}",
            w.used_percent,
            reset(w.resets_at)
        );
    }
}

fn status_json(id: Option<&Identity>, st: Option<&AccountStatus>) -> serde_json::Value {
    let window = |w: Option<&account::RateWindow>| {
        w.map(|w| {
            serde_json::json!({
                "used_percent": w.used_percent,
                "window_minutes": w.window_minutes,
                "resets_at": w.resets_at,
            })
        })
    };
    serde_json::json!({
        "email": id.and_then(|i| i.email.clone()),
        "plan": plan(id, st),
        "five_hour": window(st.and_then(|s| s.five_hour.as_ref())),
        "weekly": window(st.and_then(|s| s.weekly.as_ref())),
    })
}

fn account_status_from_usage(usage: ai_handoff_tui::account_api::UsageData) -> AccountStatus {
    AccountStatus {
        plan_type: usage.plan,
        five_hour: usage.five_hour,
        weekly: usage.weekly,
        captured_at: Some(chrono::Utc::now().timestamp_millis()),
    }
}

fn reset(resets_at: Option<i64>) -> String {
    let Some(ts) = resets_at else {
        return String::new();
    };
    let secs = ts - chrono::Utc::now().timestamp();
    if secs <= 0 {
        return " (resets now)".into();
    }
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if h > 0 {
        format!(" (resets in {h}h{m:02}m)")
    } else {
        format!(" (resets in {m}m)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_core::account::{AccountMeta, AccountSlot, RateWindow};
    use ai_handoff_tui::account_api::UsageData;
    use std::path::PathBuf;

    fn slot(label: &str, email: Option<&str>, active: bool) -> AccountSlot {
        AccountSlot {
            meta: AccountMeta {
                schema_version: 1,
                agent: "claude".into(),
                label: label.into(),
                email: email.map(String::from),
                plan_hint: None,
                account_id: None,
                workspace_id: None,
                created_at: None,
                last_verified_at: None,
                source: None,
                identity_key: None,
            },
            dir: PathBuf::new(),
            active,
        }
    }

    fn id(email: &str) -> Identity {
        Identity {
            email: Some(email.into()),
            account_id: None,
            plan_type: None,
        }
    }

    #[test]
    fn select_status_slot_requires_active_or_identity_match() {
        let slots = vec![
            slot("a@x.com", Some("a@x.com"), false),
            slot("b@x.com", Some("b@x.com"), false),
        ];
        assert!(select_status_slot(&slots, None).is_none());
        assert!(select_status_slot(&slots, Some(&id("c@x.com"))).is_none());
        assert_eq!(
            select_status_slot(&slots, Some(&id("B@X.com")))
                .unwrap()
                .meta
                .label,
            "b@x.com"
        );

        let with_active = vec![
            slot("a@x.com", Some("a@x.com"), false),
            slot("b@x.com", Some("b@x.com"), true),
        ];
        assert_eq!(
            select_status_slot(&with_active, None).unwrap().meta.label,
            "b@x.com"
        );
    }

    #[test]
    fn account_status_from_usage_maps_plan_and_windows() {
        let status = account_status_from_usage(UsageData {
            plan: Some("pro".into()),
            five_hour: Some(RateWindow {
                used_percent: 42.0,
                window_minutes: 300,
                resets_at: Some(1782864000),
            }),
            weekly: Some(RateWindow {
                used_percent: 94.0,
                window_minutes: 10080,
                resets_at: Some(1783468800),
            }),
            reset_credits: None,
            reset_credit_details: Vec::new(),
        });

        assert_eq!(status.plan_type.as_deref(), Some("pro"));
        assert_eq!(status.five_hour.unwrap().used_percent, 42.0);
        assert_eq!(status.weekly.unwrap().used_percent, 94.0);
    }
}
