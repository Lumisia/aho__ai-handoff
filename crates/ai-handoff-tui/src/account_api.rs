//! The single network call in the TUI: fetch the Codex rate-limit *reset
//! credit* count ("초기화권") from the backend.
//!
//! `codex-rs` exposes this only over an authenticated request — there is no
//! local cache — so we replay exactly what its `backend-client` does:
//!
//! ```text
//! GET https://chatgpt.com/backend-api/wham/usage
//!   Authorization: Bearer <access_token>
//!   ChatGPT-Account-Id: <account_id>
//! -> { ..., "rate_limit_reset_credits": { "available_count": <i64> } }
//! ```
//!
//! The bearer token comes from `~/.codex/auth.json` via
//! `ai_handoff_core::account::codex_request_auth` and is used **only** to set
//! the header here. It is never logged, displayed, or returned.

use std::time::Duration;

/// Codex's default ChatGPT backend. `PathStyle::ChatGptApi` (base contains
/// `/backend-api`) routes usage to `/wham/usage`, per `codex-rs`.
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

/// Fetch the number of available rate-limit reset credits for the signed-in
/// Codex account. `Err` carries a short, secret-free reason for the UI.
pub fn fetch_reset_credits() -> Result<i64, String> {
    let (access_token, account_id) =
        ai_handoff_core::account::codex_request_auth().ok_or("not signed in")?;

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(6))
        .user_agent("codex-cli")
        .build();

    let mut req = agent
        .get(USAGE_URL)
        .set("Authorization", &format!("Bearer {access_token}"));
    if let Some(acc) = account_id.as_deref() {
        req = req.set("ChatGPT-Account-Id", acc);
    }

    match req.call() {
        Ok(resp) => {
            let body: serde_json::Value = resp.into_json().map_err(|_| "bad response".to_string())?;
            Ok(available_count(&body))
        }
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err("auth expired — re-login codex".to_string())
        }
        Err(ureq::Error::Status(code, _)) => Err(format!("http {code}")),
        Err(_) => Err("network error".to_string()),
    }
}

/// Read `rate_limit_reset_credits.available_count` (absent / null => 0).
fn available_count(body: &serde_json::Value) -> i64 {
    body.get("rate_limit_reset_credits")
        .and_then(|c| c.get("available_count"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_count_reads_the_field() {
        let body = serde_json::json!({ "rate_limit_reset_credits": { "available_count": 2 } });
        assert_eq!(available_count(&body), 2);
    }

    #[test]
    fn available_count_defaults_to_zero() {
        assert_eq!(available_count(&serde_json::json!({})), 0);
        assert_eq!(
            available_count(&serde_json::json!({ "rate_limit_reset_credits": null })),
            0
        );
    }
}
