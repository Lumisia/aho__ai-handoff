//! Per-account usage from the Codex backend.
//!
//! 5-hour / weekly limits and reset credits are **per account**, so — exactly
//! like codex-auth and codex-quota — we call the usage endpoint with *each*
//! account's own token rather than reading the active account's local logs
//! (which would show the same numbers for every slot):
//!
//! ```text
//! GET https://chatgpt.com/backend-api/wham/usage
//!   Authorization: Bearer <access_token>
//!   ChatGPT-Account-Id: <account_id>
//! -> { plan_type, rate_limit: { primary_window, secondary_window },
//!      rate_limit_reset_credits: { available_count } }
//! ```
//!
//! Reset-credit expiration details are fetched from:
//!
//! ```text
//! GET https://chatgpt.com/backend-api/wham/rate-limit-reset-credits
//!   Authorization: Bearer <access_token>
//!   ChatGPT-Account-Id: <account_id>
//! -> { credits: [{ granted_at, expires_at }] }
//! ```
//!
//! The token comes from the slot's stored `auth.json` and is used only to set
//! the header here — never logged, displayed, or returned.

use std::cmp::Ordering;
use std::time::Duration;

use ai_handoff_core::account::{self, Agent, RateWindow};
use chrono::DateTime;
use serde_json::Value;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const RESET_CREDITS_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";

/// One account's usage snapshot from the backend.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageData {
    pub plan: Option<String>,
    pub five_hour: Option<RateWindow>,
    pub weekly: Option<RateWindow>,
    pub reset_credits: Option<i64>,
    pub reset_credit_details: Vec<ResetCredit>,
}

/// One reset credit returned by `wham/rate-limit-reset-credits`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResetCredit {
    pub granted_at: String,
    pub expires_at: String,
}

/// Fetch usage for a saved slot, using that slot's stored token.
pub fn fetch_slot_usage(agent: Agent, label: &str) -> Result<UsageData, String> {
    let (token, account_id) =
        account::slot_request_auth(agent, label).ok_or("no usable token in this account")?;
    fetch_usage(&token, account_id.as_deref())
}

fn fetch_usage(access_token: &str, account_id: Option<&str>) -> Result<UsageData, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(8))
        .user_agent("codex-cli")
        .build();
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
            Err("auth expired — re-add this account".to_string())
        }
        Err(ureq::Error::Status(code, _)) => Err(format!("http {code}")),
        Err(_) => Err("network error".to_string()),
    }
}

/// Parse the `wham/usage` response (field names verified against codex-auth /
/// codex-quota and codex-rs's backend models).
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
            Err("auth expired — re-add this account".to_string())
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
        assert_eq!(five.window_minutes, 300); // 18000s / 60
        assert_eq!(five.resets_at, Some(1782478701));
        let weekly = u.weekly.expect("weekly");
        assert_eq!(weekly.window_minutes, 10080); // 604800s / 60
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
