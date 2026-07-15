//! Per-account usage from provider backends.
//!
//! Saved slots have their own credentials. Slot usage must use the selected
//! slot's token, not the currently active CLI account or local session logs.
//! Live overview usage is handled elsewhere from local samples only.
//!
//! Lives in core (not the TUI) so the daemon can consult a slot's real usage at
//! hook time — the only account-scoped usage source for the five-hour trigger.

use std::cmp::Ordering;
use std::time::Duration;

use crate::account::{self, Agent, RateWindow};
use chrono::DateTime;
use serde_json::Value;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const RESET_CREDITS_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
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
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResetCredit {
    pub granted_at: String,
    pub expires_at: String,
}

/// Fetch a saved slot's usage with the default (interactive) timeouts.
pub fn fetch_slot_usage(agent: Agent, label: &str) -> Result<UsageData, String> {
    fetch_slot_usage_with(agent, label, FetchTimeouts::default())
}

/// Fetch a saved slot's usage with explicit network timeouts.
pub fn fetch_slot_usage_with(
    agent: Agent,
    label: &str,
    timeouts: FetchTimeouts,
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
            fetch_claude_usage(&token, plan, timeouts)
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
    // The active login is Claude Code's to refresh — rotating its refresh token
    // here would break the live session (forcing a browser re-login). Only
    // non-active slots (the ones that go stale from disuse) are refreshed.
    if account::claude_slot_is_active(label) {
        return Err(expired_err());
    }
    let Some(refresh_token) = account::claude_credential_refresh_token(&bytes) else {
        return Err(expired_err());
    };
    let refreshed = refresh_claude_oauth(&refresh_token, timeouts).map_err(|_| expired_err())?;
    let expires_at_ms = now_ms + refreshed.expires_in_secs.unwrap_or(0).saturating_mul(1000);
    account::persist_claude_slot_refresh(
        label,
        &refreshed.access_token,
        refreshed.refresh_token.as_deref(),
        expires_at_ms,
    )
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
    }
}

fn fetch_claude_usage(
    access_token: &str,
    plan: Option<String>,
    timeouts: FetchTimeouts,
) -> Result<UsageData, String> {
    let agent = build_agent("claude-code", timeouts);
    match fetch_claude_oauth_usage(&agent, access_token, plan) {
        Ok(usage) if claude_usage_has_windows(&usage) => Ok(usage),
        Ok(_) => Err(
            "usage response missing limits; open Claude Code and send a message to record a sample"
                .to_string(),
        ),
        Err(err) => Err(err.message()),
    }
}

fn fetch_claude_oauth_usage(
    agent: &ureq::Agent,
    access_token: &str,
    plan: Option<String>,
) -> Result<UsageData, ClaudeUsageFetchError> {
    match agent
        .get(CLAUDE_USAGE_URL)
        .set("Authorization", &format!("Bearer {access_token}"))
        .set("anthropic-beta", "oauth-2025-04-20")
        .set("Accept", "application/json")
        .call()
    {
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
    UsageData {
        plan,
        five_hour: claude_window(body, &["five_hour", "five_hour_limit"], 300),
        weekly: claude_window(body, &["seven_day", "weekly", "weekly_limit"], 10080),
        reset_credits: None,
        reset_credit_details: Vec::new(),
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
    let pct = if (0.0..=1.0).contains(&value) {
        value * 100.0
    } else {
        value
    };
    pct.clamp(0.0, 100.0)
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
            Ok(parse_reset_credit_details(&body))
        }
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err("auth expired; re-add this account".to_string())
        }
        Err(ureq::Error::Status(code, _)) => Err(format!("http {code}")),
        Err(_) => Err("network error".to_string()),
    }
}

fn parse_reset_credit_details(body: &Value) -> Vec<ResetCredit> {
    let mut credits: Vec<ResetCredit> = body
        .get("credits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|raw| {
            let granted_at = raw.get("granted_at").and_then(Value::as_str)?;
            let expires_at = raw.get("expires_at").and_then(Value::as_str)?;
            Some(ResetCredit {
                granted_at: granted_at.to_string(),
                expires_at: expires_at.to_string(),
            })
        })
        .collect();
    credits.sort_by(|a, b| {
        cmp_credit_datetime(&a.expires_at, &b.expires_at)
            .then_with(|| cmp_credit_datetime(&a.granted_at, &b.granted_at))
    });
    credits
}

fn cmp_credit_datetime(a: &str, b: &str) -> Ordering {
    match (
        DateTime::parse_from_rfc3339(a),
        DateTime::parse_from_rfc3339(b),
    ) {
        (Ok(a), Ok(b)) => a
            .timestamp()
            .cmp(&b.timestamp())
            .then_with(|| a.timestamp_subsec_nanos().cmp(&b.timestamp_subsec_nanos())),
        _ => a.cmp(b),
    }
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
        assert_eq!(five.used_percent, 42.0);
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
        assert_eq!(five.used_percent, 62.0);
        assert_eq!(five.window_minutes, 300);
        assert_eq!(five.resets_at, Some(1782864000));
        let weekly = u.weekly.expect("weekly");
        assert_eq!(weekly.used_percent, 94.0);
        assert_eq!(weekly.window_minutes, 10080);
        assert_eq!(weekly.resets_at, Some(1783468800));
    }

    #[test]
    fn parse_reset_credit_details_sorts_by_expiration() {
        let body = serde_json::json!({
            "credits": [
                { "granted_at": "2026-06-27T00:00:00Z", "expires_at": "2026-07-27T00:00:00Z" },
                { "granted_at": "2026-06-18T00:00:00Z", "expires_at": "2026-07-18T00:00:00Z" },
                { "granted_at": "2026-06-18T00:00:01Z", "expires_at": "2026-07-18T09:00:00+09:00" },
                { "granted_at": "2026-06-19T00:00:00Z", "expires_at": "2026-07-19T00:00:00Z" },
                { "granted_at": "bad" },
                null
            ],
            "rate_limit_reset_credits": { "available_count": 4 }
        });

        let credits = parse_reset_credit_details(&body);
        assert_eq!(credits.len(), 4);
        assert_eq!(
            credits,
            vec![
                ResetCredit {
                    granted_at: "2026-06-18T00:00:00Z".into(),
                    expires_at: "2026-07-18T00:00:00Z".into(),
                },
                ResetCredit {
                    granted_at: "2026-06-18T00:00:01Z".into(),
                    expires_at: "2026-07-18T09:00:00+09:00".into(),
                },
                ResetCredit {
                    granted_at: "2026-06-19T00:00:00Z".into(),
                    expires_at: "2026-07-19T00:00:00Z".into(),
                },
                ResetCredit {
                    granted_at: "2026-06-27T00:00:00Z".into(),
                    expires_at: "2026-07-27T00:00:00Z".into(),
                },
            ]
        );
    }
}
