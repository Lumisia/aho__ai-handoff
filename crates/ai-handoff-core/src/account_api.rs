//! Per-account usage from provider backends.
//!
//! Saved slots have their own credentials. Slot usage must use the selected
//! slot's token, not the currently active CLI account or local session logs.
//! Live overview usage is handled elsewhere from local samples only.
//!
//! Lives in core (not the TUI) so the daemon can consult a slot's real usage at
//! hook time — the only account-scoped usage source for the five-hour trigger.

use std::time::Duration;

use crate::account::{self, Agent, RateWindow};
use chrono::{DateTime, FixedOffset, Utc};
use serde_json::Value;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const RESET_CREDITS_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Compatibility identity observed in OpenUsage's published implementation.
/// Keep this confined to interactive inventory reads; see docs/advanced/claude-reset-credits.md.
const CLAUDE_RESET_USER_AGENT: &str = "claude-cli/2.1.294 (external, cli)";
/// Claude Code's public OAuth token endpoint and client id (mechanical
/// refresh-token grant only — the interactive login flow still belongs to the
/// official CLI). Values match the installed Claude Code CLI.
const CLAUDE_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLAUDE_OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// A saved access token expiring within this margin is treated as already
/// stale, so a token dying mid-request is refreshed ahead of time.
const CLAUDE_TOKEN_REFRESH_MARGIN_MS: i64 = 60_000;

/// Network timeouts for a usage fetch. The interactive GUI/TUI can afford the
/// generous default; the daemon's hook-time fetch passes a short budget so it
/// fits inside the 10s hook timeout.
#[derive(Clone, Copy, Debug)]
pub struct FetchTimeouts {
    pub connect: Duration,
    pub read: Duration,
}

impl Default for FetchTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(4),
            read: Duration::from_secs(8),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageData {
    pub plan: Option<String>,
    pub five_hour: Option<RateWindow>,
    pub weekly: Option<RateWindow>,
    pub reset_credits: Option<i64>,
    pub reset_credit_details: Vec<ResetCredit>,
    /// None means the provider did not expose a trustworthy inventory.
    pub claude_reset_credits: Option<Vec<ClaudeResetCredit>>,
}

/// Display-only data: redemption handles and provider text never leave the parser.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ClaudeResetCredit {
    pub remaining: i64,
    pub starts_at: Option<i64>,
    pub expires_at: Option<i64>,
    pub kind: ClaudeResetKind,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeResetKind {
    Full,
    Weekly,
    Session,
    Scoped,
    Other,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResetCredit {
    pub granted_at: Option<DateTime<FixedOffset>>,
    pub expires_at: Option<DateTime<FixedOffset>>,
    pub kind: CodexResetKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexResetKind {
    Full,
    #[default]
    Other,
}

/// Fetch a saved slot's usage with the default (interactive) timeouts.
pub fn fetch_slot_usage(agent: Agent, label: &str) -> Result<UsageData, String> {
    fetch_slot_usage_impl(agent, label, FetchTimeouts::default(), true)
}

/// Fetch usage for a latency-sensitive caller without optional Claude inventory.
pub fn fetch_slot_usage_with(
    agent: Agent,
    label: &str,
    timeouts: FetchTimeouts,
) -> Result<UsageData, String> {
    fetch_slot_usage_impl(agent, label, timeouts, false)
}

fn fetch_slot_usage_impl(
    agent: Agent,
    label: &str,
    timeouts: FetchTimeouts,
    include_reset_credits: bool,
) -> Result<UsageData, String> {
    match agent {
        Agent::Codex => {
            let (token, account_id) = account::slot_request_auth(agent, label)
                .ok_or("no usable token in this account")?;
            fetch_usage(&token, account_id.as_deref(), timeouts)
        }
        Agent::Claude => {
            // Re-syncs the active slot from the (fresher) live credential, then
            // — if the saved token has still expired — exchanges the stored
            // refresh token for a new one so a non-active slot stays usable.
            let (token, plan) = claude_token_refreshing(label, timeouts)?;
            fetch_claude_usage(&token, plan, timeouts, include_reset_credits)
        }
    }
}

fn build_agent(user_agent: &str, timeouts: FetchTimeouts) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(timeouts.connect)
        .timeout_read(timeouts.read)
        .user_agent(user_agent)
        .build()
}

/// A saved Claude slot's usable access token, refreshing it first if the stored
/// one has expired. The active-slot live re-sync happens inside
/// [`account::claude_slot_synced_bytes`]; only a still-expired token (a slot
/// that is not the current login) triggers the network refresh grant.
fn claude_token_refreshing(
    label: &str,
    timeouts: FetchTimeouts,
) -> Result<(String, Option<String>), String> {
    let bytes = account::claude_slot_synced_bytes(label)
        .ok_or_else(|| "no usable token in this account".to_string())?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let fresh_enough = account::claude_credential_expires_at(&bytes)
        .is_none_or(|expires_at| expires_at > now_ms + CLAUDE_TOKEN_REFRESH_MARGIN_MS);
    if fresh_enough {
        return account::claude_oauth_pair(&bytes)
            .ok_or_else(|| "no usable token in this account".to_string());
    }
    // The stored access token has expired; exchange the refresh token for a new
    // one and persist it so the slot keeps working without a manual re-add.
    let expired_err =
        || "token expired; launch this account (l) or open Claude Code to refresh it".to_string();
    let Some(refresh_token) = account::claude_credential_refresh_token(&bytes) else {
        return Err(expired_err());
    };
    // When this slot IS the live login (byte-identical — Claude Code left the
    // live credential expired), the rotated refresh token must be written back
    // to the live file too, or Claude Code's stored refresh token is
    // invalidated by this very refresh and its next start forces a re-login.
    let live_was_identical = account::claude_live_credential_equals(&bytes);
    let refreshed = refresh_claude_oauth(&refresh_token, timeouts).map_err(|_| expired_err())?;
    let expires_at_ms = now_ms + refreshed.expires_in_secs.unwrap_or(0).saturating_mul(1000);
    let persisted = account::persist_claude_slot_refresh(
        label,
        &bytes,
        &refreshed.access_token,
        refreshed.refresh_token.as_deref(),
        expires_at_ms,
    )?;
    if live_was_identical {
        // Best-effort: a failed propagation must not fail the usage fetch —
        // the slot itself now holds a valid token either way.
        let _ = account::propagate_claude_slot_to_live(label, &bytes, &persisted.0);
    }
    Ok(persisted)
}

/// The subset of an OAuth refresh response we persist.
struct RefreshedClaudeToken {
    access_token: String,
    /// Present when the provider rotates the refresh token; when absent the old
    /// one stays valid and is kept.
    refresh_token: Option<String>,
    expires_in_secs: Option<i64>,
}

/// Exchange a Claude OAuth refresh token for a fresh access token via the
/// public token endpoint. No credential is read or written here — the caller
/// owns persistence.
fn refresh_claude_oauth(
    refresh_token: &str,
    timeouts: FetchTimeouts,
) -> Result<RefreshedClaudeToken, String> {
    let agent = build_agent("claude-code", timeouts);
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLAUDE_OAUTH_CLIENT_ID,
    });
    match agent
        .post(CLAUDE_TOKEN_URL)
        .set("anthropic-beta", "oauth-2025-04-20")
        .set("Accept", "application/json")
        .send_json(body)
    {
        Ok(resp) => {
            let value: Value = resp
                .into_json()
                .map_err(|_| "bad refresh response".to_string())?;
            let access_token = value
                .get("access_token")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
                .ok_or_else(|| "refresh response missing access_token".to_string())?
                .to_string();
            let refresh_token = value
                .get("refresh_token")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
                .map(String::from);
            let expires_in_secs = value.get("expires_in").and_then(Value::as_i64);
            Ok(RefreshedClaudeToken {
                access_token,
                refresh_token,
                expires_in_secs,
            })
        }
        Err(ureq::Error::Status(code, _)) => Err(format!("refresh failed: http {code}")),
        Err(_) => Err("refresh failed: network error".to_string()),
    }
}

fn fetch_usage(
    access_token: &str,
    account_id: Option<&str>,
    timeouts: FetchTimeouts,
) -> Result<UsageData, String> {
    let agent = build_agent("codex-cli", timeouts);
    let mut req = agent
        .get(USAGE_URL)
        .set("Authorization", &format!("Bearer {access_token}"));
    if let Some(acc) = account_id {
        req = req.set("ChatGPT-Account-Id", acc);
    }
    match req.call() {
        Ok(resp) => {
            let body: Value = resp.into_json().map_err(|_| "bad response".to_string())?;
            let mut usage = parse_usage(&body);
            if usage.reset_credits.unwrap_or(0) > 0 {
                if let Some(acc) = account_id {
                    if let Ok(details) = fetch_reset_credit_details(&agent, access_token, acc) {
                        usage.reset_credit_details = details;
                    }
                }
            }
            Ok(usage)
        }
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err("auth expired; re-add this account".to_string())
        }
        Err(ureq::Error::Status(code, _)) => Err(format!("http {code}")),
        Err(_) => Err("network error".to_string()),
    }
}

fn parse_usage(body: &Value) -> UsageData {
    let rate_limit = body.get("rate_limit");
    let window = |name: &str| -> Option<RateWindow> {
        let w = rate_limit?.get(name)?;
        let used_percent = w.get("used_percent")?.as_f64()?;
        let window_minutes = w
            .get("limit_window_seconds")
            .and_then(Value::as_u64)
            .map(|s| s / 60)
            .unwrap_or(0);
        let resets_at = w.get("reset_at").and_then(Value::as_i64);
        Some(RateWindow {
            used_percent,
            window_minutes,
            resets_at,
        })
    };
    UsageData {
        plan: body
            .get("plan_type")
            .and_then(Value::as_str)
            .map(String::from),
        five_hour: window("primary_window"),
        weekly: window("secondary_window"),
        reset_credits: body
            .get("rate_limit_reset_credits")
            .and_then(|c| c.get("available_count"))
            .and_then(Value::as_i64),
        reset_credit_details: Vec::new(),
        claude_reset_credits: None,
    }
}

fn fetch_claude_usage(
    access_token: &str,
    plan: Option<String>,
    timeouts: FetchTimeouts,
    include_reset_credits: bool,
) -> Result<UsageData, String> {
    // A bearer-authenticated usage read must stay on its fixed HTTPS endpoint.
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeouts.connect)
        .timeout_read(timeouts.read)
        .user_agent("claude-code")
        .https_only(true)
        .redirects(0)
        .build();
    match fetch_claude_oauth_usage_using(
        &agent,
        access_token,
        plan,
        include_reset_credits,
        |request| request.call(),
    ) {
        Ok(usage) if claude_usage_has_windows(&usage) => Ok(usage),
        Ok(_) => Err(
            "usage response missing limits; open Claude Code and send a message to record a sample"
                .to_string(),
        ),
        Err(err) => Err(err.message()),
    }
}

fn fetch_claude_oauth_usage_using(
    agent: &ureq::Agent,
    access_token: &str,
    plan: Option<String>,
    include_reset_credits: bool,
    mut send: impl FnMut(ureq::Request) -> Result<ureq::Response, ureq::Error>,
) -> Result<UsageData, ClaudeUsageFetchError> {
    let request = |inventory| {
        agent
            .get(if inventory {
                "https://api.anthropic.com/api/oauth/usage?cedar_ember=1"
            } else {
                CLAUDE_USAGE_URL
            })
            .set("Authorization", &format!("Bearer {access_token}"))
            .set("anthropic-beta", "oauth-2025-04-20")
            .set("Accept", "application/json")
            .set(
                "User-Agent",
                if inventory {
                    CLAUDE_RESET_USER_AGENT
                } else {
                    "claude-code"
                },
            )
    };
    let mut response = send(request(include_reset_credits));
    if include_reset_credits && matches!(response, Err(ureq::Error::Status(400 | 403, _))) {
        // Optional inventory rejection must not break existing quota reporting.
        // Same credential, same origin, once; never retry 401/429 or network errors.
        response = send(request(false));
    }
    match response {
        Ok(resp) => {
            let body: Value = resp
                .into_json()
                .map_err(|_| ClaudeUsageFetchError::Other("bad response".to_string()))?;
            Ok(parse_claude_usage(&body, plan))
        }
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err(ClaudeUsageFetchError::AuthExpired)
        }
        Err(ureq::Error::Status(429, resp)) => {
            let retry = resp.header("retry-after").unwrap_or("later");
            Err(ClaudeUsageFetchError::RateLimited(retry.to_string()))
        }
        Err(ureq::Error::Status(code, _)) => {
            Err(ClaudeUsageFetchError::Other(format!("http {code}")))
        }
        Err(_) => Err(ClaudeUsageFetchError::Other("network error".to_string())),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClaudeUsageFetchError {
    AuthExpired,
    RateLimited(String),
    Other(String),
}

impl ClaudeUsageFetchError {
    fn message(self) -> String {
        match self {
            Self::AuthExpired => "auth expired; re-add this account".to_string(),
            Self::RateLimited(retry) => format!("rate limited; retry {retry}"),
            Self::Other(message) => message,
        }
    }
}

fn claude_usage_has_windows(usage: &UsageData) -> bool {
    usage.five_hour.is_some() || usage.weekly.is_some()
}

fn parse_claude_usage(body: &Value, plan: Option<String>) -> UsageData {
    parse_claude_usage_at(body, plan, chrono::Utc::now().timestamp())
}

fn parse_claude_usage_at(body: &Value, plan: Option<String>, now: i64) -> UsageData {
    let credits = parse_claude_reset_credits(body, now);
    UsageData {
        plan,
        five_hour: claude_window(body, &["five_hour", "five_hour_limit"], 300),
        weekly: claude_window(body, &["seven_day", "weekly", "weekly_limit"], 10080),
        reset_credits: credits
            .as_ref()
            .map(|rows| rows.iter().map(|row| row.remaining).sum()),
        reset_credit_details: Vec::new(),
        claude_reset_credits: credits,
    }
}

fn parse_claude_reset_credits(body: &Value, now: i64) -> Option<Vec<ClaudeResetCredit>> {
    let inventory = body.get("cedar_ember")?;
    // Surface/version restrictions do not establish an empty balance.
    if inventory.get("eligible").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let grants = inventory.get("grants")?.as_array()?;
    if grants.len() > 1000 {
        return None;
    }
    let mut credits = Vec::new();
    let mut total = 0i64;
    for grant in grants {
        let remaining = grant.get("resets_left")?.as_i64()?;
        let granted = grant.get("resets_total")?.as_i64()?;
        if !(0..=1000).contains(&remaining) || granted < remaining {
            return None;
        }
        let starts_at = optional_claude_credit_time(grant.get("starts_at"))?;
        let expires_at = optional_claude_credit_time(grant.get("ends_at"))?;
        let paused = grant.get("paused")?.as_bool()?;
        if remaining == 0
            || paused
            || starts_at.is_some_and(|start| start > now)
            || expires_at.is_some_and(|end| end <= now)
        {
            continue;
        }
        total = total.checked_add(remaining)?;
        if total > 1000 {
            return None;
        }
        let clears = grant.get("clears").and_then(Value::as_array);
        let clears_window = |name: &str| {
            clears.is_some_and(|values| values.iter().any(|v| v.as_str() == Some(name)))
        };
        let scoped = grant.get("gives_back_weekly").and_then(Value::as_bool) == Some(true);
        let kind = if scoped
            || (clears_window("seven_day_overage_included") && !clears_window("seven_day"))
        {
            ClaudeResetKind::Scoped
        } else if clears_window("five_hour") && clears_window("seven_day") {
            ClaudeResetKind::Full
        } else if clears_window("seven_day") {
            ClaudeResetKind::Weekly
        } else if clears_window("five_hour") {
            ClaudeResetKind::Session
        } else {
            ClaudeResetKind::Other
        };
        // `usable_now` describes redemption gates, not how many resets are saved.
        credits.push(ClaudeResetCredit {
            remaining,
            starts_at,
            expires_at,
            kind,
        });
    }
    credits.sort_by_key(|credit| credit.expires_at.unwrap_or(i64::MAX));
    Some(credits)
}

fn optional_claude_credit_time(value: Option<&Value>) -> Option<Option<i64>> {
    match value {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(value)) => DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|date| Some(date.timestamp())),
        _ => None,
    }
}

fn claude_window(body: &Value, keys: &[&str], window_minutes: u64) -> Option<RateWindow> {
    let raw = keys
        .iter()
        .find_map(|key| body.get(*key))
        .or_else(|| {
            body.get("rate_limits")
                .and_then(|rl| keys.iter().find_map(|key| rl.get(*key)))
        })
        .or_else(|| claude_limit_array_item(body, keys))?;
    let direct_used = raw
        .get("utilization")
        .or_else(|| raw.get("used_percentage"))
        .or_else(|| raw.get("percent"))
        .or_else(|| raw.get("percentage"))
        .or_else(|| raw.get("usage"))
        .and_then(Value::as_f64)
        .map(normalize_claude_percent);
    let used_percent = direct_used.or_else(|| {
        raw.get("percent_remaining")
            .and_then(Value::as_f64)
            .map(|remaining| 100.0 - normalize_claude_percent(remaining))
    })?;
    let resets_at = raw
        .get("resets_at")
        .or_else(|| raw.get("resetsAt"))
        .or_else(|| raw.get("reset_at"))
        .and_then(parse_claude_reset);
    Some(RateWindow {
        used_percent,
        window_minutes,
        resets_at,
    })
}

fn claude_limit_array_item<'a>(body: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    body.get("limits")
        .or_else(|| body.get("quotas"))
        .or_else(|| body.get("rate_limits"))?
        .as_array()?
        .iter()
        .find(|item| {
            item.get("kind")
                .or_else(|| item.get("id"))
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
                .is_some_and(|name| keys.contains(&name))
        })
}

fn normalize_claude_percent(value: f64) -> f64 {
    // OAuth utilization and the percentage aliases are already in 0..100 units.
    // Guessing a fraction from a value <= 1 turns real 1% usage into 100%.
    value.clamp(0.0, 100.0)
}

fn parse_claude_reset(value: &Value) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        return Some(n);
    }
    let s = value.as_str()?;
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp())
}

fn fetch_reset_credit_details(
    agent: &ureq::Agent,
    access_token: &str,
    account_id: &str,
) -> Result<Vec<ResetCredit>, String> {
    match agent
        .get(RESET_CREDITS_URL)
        .set("Authorization", &format!("Bearer {access_token}"))
        .set("ChatGPT-Account-Id", account_id)
        .call()
    {
        Ok(resp) => {
            let body: Value = resp.into_json().map_err(|_| "bad response".to_string())?;
            Ok(parse_reset_credit_details(&body, Utc::now()))
        }
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err("auth expired; re-add this account".to_string())
        }
        Err(ureq::Error::Status(code, _)) => Err(format!("http {code}")),
        Err(_) => Err("network error".to_string()),
    }
}

fn parse_reset_credit_details(body: &Value, now: DateTime<Utc>) -> Vec<ResetCredit> {
    let mut credits: Vec<ResetCredit> = body
        .get("credits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|raw| {
            let date = |field| {
                raw.get(field)
                    .and_then(Value::as_str)
                    .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            };
            let granted_at = date("granted_at");
            let expires_at = date("expires_at");
            match raw.get("status") {
                Some(status) if status.as_str() == Some("available") => {}
                // Older responses could omit status and only carry the dates.
                None if granted_at.is_some() && expires_at.is_some() => {}
                _ => return None,
            }
            if expires_at.is_some_and(|expires| expires <= now) {
                return None;
            }
            let kind = match raw.get("reset_type").and_then(Value::as_str) {
                Some("codex_rate_limits") => CodexResetKind::Full,
                _ => CodexResetKind::Other,
            };
            Some(ResetCredit {
                granted_at,
                expires_at,
                kind,
            })
        })
        .collect();
    credits.sort_by_key(|credit| {
        (
            credit.expires_at.is_none(),
            credit.expires_at,
            credit.granted_at,
        )
    });
    credits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_usage_reads_windows_plan_and_credits() {
        let body = serde_json::json!({
            "plan_type": "team",
            "rate_limit": {
                "primary_window": { "used_percent": 100.0, "limit_window_seconds": 18000, "reset_at": 1782478701i64 },
                "secondary_window": { "used_percent": 87.0, "limit_window_seconds": 604800, "reset_at": 1782808275i64 }
            },
            "rate_limit_reset_credits": { "available_count": 2 }
        });
        let u = parse_usage(&body);
        assert_eq!(u.plan.as_deref(), Some("team"));
        let five = u.five_hour.expect("5h");
        assert_eq!(five.used_percent, 100.0);
        assert_eq!(five.window_minutes, 300);
        assert_eq!(five.resets_at, Some(1782478701));
        let weekly = u.weekly.expect("weekly");
        assert_eq!(weekly.window_minutes, 10080);
        assert_eq!(u.reset_credits, Some(2));
    }

    #[test]
    fn parse_usage_tolerates_missing_fields() {
        let u = parse_usage(&serde_json::json!({}));
        assert!(u.plan.is_none());
        assert!(u.five_hour.is_none());
        assert!(u.weekly.is_none());
        assert!(u.reset_credits.is_none());
    }

    #[test]
    fn parse_claude_usage_reads_five_hour_and_weekly() {
        let body = serde_json::json!({
            "five_hour": { "utilization": 0.42, "resets_at": "2026-07-01T00:00:00Z" },
            "seven_day": { "utilization": 11.0, "resets_at": "2026-07-08T00:00:00Z" }
        });

        let u = parse_claude_usage(&body, Some("pro".into()));

        assert_eq!(u.plan.as_deref(), Some("pro"));
        let five = u.five_hour.expect("5h");
        assert_eq!(five.used_percent, 0.42);
        assert_eq!(five.window_minutes, 300);
        assert_eq!(five.resets_at, Some(1782864000));
        let weekly = u.weekly.expect("weekly");
        assert_eq!(weekly.used_percent, 11.0);
        assert_eq!(weekly.window_minutes, 10080);
        assert_eq!(weekly.resets_at, Some(1783468800));
    }

    #[test]
    fn parse_claude_usage_reads_nested_rate_limits() {
        let body = serde_json::json!({
            "rate_limits": {
                "five_hour_limit": {
                    "utilization": 0.62,
                    "resets_at": "2026-07-01T00:00:00Z"
                },
                "weekly_limit": {
                    "utilization": 94.0,
                    "resets_at": "2026-07-08T00:00:00Z"
                }
            }
        });

        let u = parse_claude_usage(&body, Some("pro".into()));

        assert_eq!(u.plan.as_deref(), Some("pro"));
        let five = u.five_hour.expect("5h");
        assert_eq!(five.used_percent, 0.62);
        assert_eq!(five.window_minutes, 300);
        assert_eq!(five.resets_at, Some(1782864000));
        let weekly = u.weekly.expect("weekly");
        assert_eq!(weekly.used_percent, 94.0);
        assert_eq!(weekly.window_minutes, 10080);
        assert_eq!(weekly.resets_at, Some(1783468800));
    }

    #[test]
    fn claude_low_usage_percentages_are_not_inferred_as_fractions() {
        for percent in [0.0, 0.42, 1.0, 1.01, 42.0, 100.0] {
            let body = serde_json::json!({
                "five_hour": { "utilization": percent },
                "seven_day": { "used_percentage": percent },
            });
            let usage = parse_claude_usage(&body, None);
            assert_eq!(usage.five_hour.unwrap().used_percent, percent);
            assert_eq!(usage.weekly.unwrap().used_percent, percent);
        }
        let usage = parse_claude_usage(
            &serde_json::json!({"five_hour": {"percent_remaining": 1.0}}),
            None,
        );
        assert_eq!(usage.five_hour.unwrap().used_percent, 99.0);
    }

    fn claude_reset_body(grants: Value) -> Value {
        serde_json::json!({
            "five_hour": { "utilization": 42 },
            "cedar_ember": { "eligible": true, "grants": grants, "next_grant_id": "private-handle" }
        })
    }

    fn claude_grant() -> Value {
        serde_json::json!({
            "id": "private-handle", "label": "untrusted label",
            "resets_left": 2, "resets_total": 3,
            "starts_at": "2026-11-01T00:00:00Z", "ends_at": "2026-12-11T22:00:00Z",
            "paused": false, "usable_now": false, "use_requires_limit": true,
            "clears": ["five_hour", "seven_day"], "type": "full", "gives_back_weekly": false
        })
    }

    #[test]
    fn claude_resets_sum_remaining_counts_and_exclude_redemption_identifiers() {
        let body = claude_reset_body(serde_json::json!([claude_grant()]));
        let usage = parse_claude_usage_at(&body, Some("pro".into()), 1794787200);
        assert_eq!(usage.reset_credits, Some(2));
        let credits = usage.claude_reset_credits.unwrap();
        assert_eq!(credits.len(), 1);
        assert_eq!(credits[0].remaining, 2);
        assert_eq!(credits[0].kind, ClaudeResetKind::Full);
        assert!(credits[0].expires_at.is_some());
        let serialized = serde_json::to_string(&credits).unwrap();
        assert!(!serialized.contains("private-handle"));
        assert!(!serialized.contains("untrusted label"));
        assert!(usage.reset_credit_details.is_empty());
    }

    #[test]
    fn claude_resets_filter_inactive_grants_at_exact_time_boundaries() {
        let now = DateTime::parse_from_rfc3339("2026-11-16T00:00:00Z")
            .unwrap()
            .timestamp();
        let mut expired = claude_grant();
        expired["ends_at"] = "2026-11-16T00:00:00Z".into();
        let mut future = claude_grant();
        future["starts_at"] = "2026-11-16T00:00:01Z".into();
        let mut paused = claude_grant();
        paused["paused"] = true.into();
        let mut exhausted = claude_grant();
        exhausted["resets_left"] = 0.into();
        let mut active = claude_grant();
        active["starts_at"] = "2026-11-16T00:00:00Z".into();
        let body = claude_reset_body(serde_json::json!([
            expired, future, paused, exhausted, active
        ]));
        let credits = parse_claude_reset_credits(&body, now).unwrap();
        assert_eq!(credits.len(), 1);
        assert_eq!(credits[0].remaining, 2);
    }

    #[test]
    fn claude_resets_distinguish_unknown_from_known_empty_inventory() {
        for body in [
            serde_json::json!({}),
            serde_json::json!({"cedar_ember": null}),
            serde_json::json!({"cedar_ember": {"eligible": false, "ineligible_reason": "surface", "grants": []}}),
            serde_json::json!({"cedar_ember": {"eligible": true}}),
        ] {
            assert!(parse_claude_usage_at(&body, None, 1794787200)
                .reset_credits
                .is_none());
        }
        let usage =
            parse_claude_usage_at(&claude_reset_body(serde_json::json!([])), None, 1794787200);
        assert_eq!(usage.reset_credits, Some(0));
        assert_eq!(usage.claude_reset_credits, Some(vec![]));
    }

    #[test]
    fn claude_resets_reject_malformed_inventory_without_losing_usage_windows() {
        for (field, value) in [
            ("resets_left", serde_json::json!(-1)),
            ("resets_left", serde_json::json!(1.5)),
            ("resets_left", serde_json::json!("2")),
            ("resets_left", serde_json::json!(1001)),
            ("resets_left", serde_json::json!(4)),
            ("ends_at", serde_json::json!("private-not-a-date")),
            ("starts_at", serde_json::json!(55)),
            ("paused", serde_json::json!("false")),
        ] {
            let mut grant = claude_grant();
            grant[field] = value;
            let usage = parse_claude_usage_at(
                &claude_reset_body(serde_json::json!([grant])),
                None,
                1794787200,
            );
            assert!(usage.reset_credits.is_none(), "field {field}");
            assert!(usage.five_hour.is_some());
        }
    }

    #[test]
    fn claude_resets_do_not_mislabel_model_specific_full_type() {
        let mut grant = claude_grant();
        grant["gives_back_weekly"] = true.into();
        grant["clears"] = serde_json::json!(["five_hour", "seven_day_overage_included"]);
        grant["starts_at"] = Value::Null;
        grant["ends_at"] = Value::Null;
        let usage = parse_claude_usage_at(
            &claude_reset_body(serde_json::json!([grant])),
            None,
            1794787200,
        );
        let credits = usage.claude_reset_credits.unwrap();
        assert_eq!(credits[0].kind, ClaudeResetKind::Scoped);
        assert_eq!(credits[0].expires_at, None);
        assert_eq!(credits[0].starts_at, None);
    }

    #[test]
    fn claude_inventory_request_uses_cli_compatibility_without_extra_spend_or_redemption() {
        let agent = build_agent("claude-code", FetchTimeouts::default());
        let mut calls = 0;
        let usage = fetch_claude_oauth_usage_using(&agent, "test-token", None, true, |request| {
            calls += 1;
            assert_eq!(request.method(), "GET");
            assert_eq!(
                request.url(),
                "https://api.anthropic.com/api/oauth/usage?cedar_ember=1"
            );
            assert_eq!(request.header("Authorization"), Some("Bearer test-token"));
            assert_eq!(
                request.header("User-Agent"),
                Some("claude-cli/2.1.294 (external, cli)")
            );
            assert_eq!(request.header("anthropic-beta"), Some("oauth-2025-04-20"));
            Ok(ureq::Response::new(
                200,
                "OK",
                r#"{"five_hour":{"utilization":42},"cedar_ember":{"eligible":true,"grants":[]}}"#,
            )
            .unwrap())
        })
        .unwrap();
        assert_eq!(calls, 1);
        assert!(usage.five_hour.is_some());
        assert_eq!(usage.reset_credits, Some(0));
    }

    #[test]
    fn claude_optional_inventory_rejection_falls_back_once_with_same_account() {
        let agent = build_agent("claude-code", FetchTimeouts::default());
        for status in [400, 403, 401, 429] {
            let mut calls = 0;
            let result =
                fetch_claude_oauth_usage_using(&agent, "test-token", None, true, |request| {
                    calls += 1;
                    assert_eq!(request.header("Authorization"), Some("Bearer test-token"));
                    if calls == 1 {
                        return Err(ureq::Error::Status(
                            status,
                            ureq::Response::new(status, "error", "private-provider-error").unwrap(),
                        ));
                    }
                    assert_eq!(request.url(), "https://api.anthropic.com/api/oauth/usage");
                    assert_eq!(request.header("User-Agent"), Some("claude-code"));
                    Ok(
                        ureq::Response::new(200, "OK", r#"{"five_hour":{"utilization":42}}"#)
                            .unwrap(),
                    )
                });
            assert_eq!(calls, if status == 400 || status == 403 { 2 } else { 1 });
            if status == 400 || status == 403 {
                let usage = result.unwrap();
                assert!(usage.five_hour.is_some());
                assert_eq!(usage.reset_credits, None);
            } else {
                assert!(!result
                    .unwrap_err()
                    .message()
                    .contains("private-provider-error"));
            }
        }
    }

    #[test]
    fn claude_hook_fetch_does_not_opt_in_to_inventory_or_retry() {
        let agent = build_agent("claude-code", FetchTimeouts::default());
        let mut calls = 0;
        let result = fetch_claude_oauth_usage_using(&agent, "test-token", None, false, |request| {
            calls += 1;
            assert_eq!(request.url(), "https://api.anthropic.com/api/oauth/usage");
            assert_eq!(request.header("User-Agent"), Some("claude-code"));
            Err(ureq::Error::Status(
                400,
                ureq::Response::new(400, "error", "").unwrap(),
            ))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn parse_reset_credit_details_lists_available_cards_with_missing_expiration() {
        let body = serde_json::json!({
            "credits": [
                { "status": "redeemed", "granted_at": "2026-06-17T00:00:00Z", "expires_at": "2099-07-17T00:00:00Z" },
                { "status": "available", "reset_type": "codex_rate_limits", "granted_at": "2026-06-18T00:00:00Z", "expires_at": null },
                { "status": "available", "granted_at": "2026-06-19T00:00:00Z", "expires_at": "2026-06-21T00:00:00Z" },
                { "status": "redeeming", "granted_at": "2026-06-20T00:00:00Z", "expires_at": "2099-07-18T00:00:00Z" },
            ],
            "available_count": 1
        });

        let credits = parse_reset_credit_details(&body, "2026-06-21T00:00:00Z".parse().unwrap());
        assert_eq!(
            credits,
            vec![ResetCredit {
                granted_at: Some("2026-06-18T00:00:00Z".parse().unwrap()),
                expires_at: None,
                kind: CodexResetKind::Full,
            }]
        );
    }

    #[test]
    fn parse_reset_credit_details_sorts_by_expiration() {
        let body = serde_json::json!({
            "credits": [
                { "status": "available", "reset_type": "codex_rate_limits", "granted_at": "2026-06-27T00:00:00Z", "expires_at": "2026-07-27T00:00:00Z" },
                { "granted_at": "2026-06-18T00:00:00Z", "expires_at": "2026-07-18T00:00:00Z" },
                { "granted_at": "2026-06-18T00:00:01Z", "expires_at": "2026-07-18T09:00:00+09:00" },
                { "status": "available", "reset_type": "future_reset_type", "title": "Full reset", "granted_at": "2026-06-19T00:00:00Z", "expires_at": "2026-07-19T00:00:00Z" },
                { "status": "available", "granted_at": "bad", "expires_at": "bad" },
                { "granted_at": "bad" },
                null
            ],
            "available_count": 5
        });

        let credits = parse_reset_credit_details(&body, "2026-06-21T00:00:00Z".parse().unwrap());
        assert_eq!(
            credits,
            vec![
                ResetCredit {
                    granted_at: Some("2026-06-18T00:00:00Z".parse().unwrap()),
                    expires_at: Some("2026-07-18T00:00:00Z".parse().unwrap()),
                    kind: CodexResetKind::Other,
                },
                ResetCredit {
                    granted_at: Some("2026-06-18T00:00:01Z".parse().unwrap()),
                    expires_at: Some("2026-07-18T09:00:00+09:00".parse().unwrap()),
                    kind: CodexResetKind::Other,
                },
                ResetCredit {
                    granted_at: Some("2026-06-19T00:00:00Z".parse().unwrap()),
                    expires_at: Some("2026-07-19T00:00:00Z".parse().unwrap()),
                    kind: CodexResetKind::Other,
                },
                ResetCredit {
                    granted_at: Some("2026-06-27T00:00:00Z".parse().unwrap()),
                    expires_at: Some("2026-07-27T00:00:00Z".parse().unwrap()),
                    kind: CodexResetKind::Full,
                },
                ResetCredit::default(),
            ]
        );
    }
}
