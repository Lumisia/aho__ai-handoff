//! Account status for the connected agents (Codex / Claude) and a small
//! "credential pool" that swaps which saved auth file is active.
//!
//! Read-only and local — **no network** lives here (the Codex reset-credit
//! count, which needs an authenticated backend call, is in the TUI's
//! `account_api` module). Everything here reads files the agents already wrote:
//!
//! - Codex 5-hour / weekly limits + plan: the latest `~/.codex/sessions/**`
//!   rollout line carries `payload.rate_limits` (`primary` = 5h, `secondary` =
//!   weekly), verified against real rollout files and `codex-rs`.
//! - Codex account email / plan / id: the `id_token` JWT inside
//!   `~/.codex/auth.json` (`tokens.id_token`), decoded locally. Raw tokens are
//!   only read from saved slot files by the TUI's network module; they are
//!   never logged.
//! - Claude account email: `~/.claude.json` `oauthAccount.emailAddress`
//!   (config, not a credential file).
//!
//! The pool stores copies of the agents' auth files under
//! `<AI_HANDOFF_HOME>/accounts/<agent>/<label>.authsnap`; switching copies a
//! snapshot over the live auth file (the user-approved file-swap mechanism).

use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Which connected agent an account belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Codex,
    Claude,
}

impl Agent {
    fn dir(self) -> &'static str {
        match self {
            Agent::Codex => "codex",
            Agent::Claude => "claude",
        }
    }
}

/// One rate-limit window: how much is used and when it resets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RateWindow {
    pub used_percent: f64,
    /// Window length in minutes (300 = 5h, 10080 = weekly).
    pub window_minutes: u64,
    /// Unix seconds when the window resets, if known.
    pub resets_at: Option<i64>,
}

impl RateWindow {
    /// Remaining percent (clamped to 0..=100).
    pub fn remaining_percent(&self) -> f64 {
        (100.0 - self.used_percent).clamp(0.0, 100.0)
    }
}

/// A live usage snapshot for one agent (plan + the two windows).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountStatus {
    pub plan_type: Option<String>,
    pub five_hour: Option<RateWindow>,
    pub weekly: Option<RateWindow>,
    /// Unix milliseconds the sample was captured, if known.
    pub captured_at: Option<i64>,
}

/// Who is logged in for an agent (no secrets — display fields only).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Identity {
    pub email: Option<String>,
    pub account_id: Option<String>,
    pub plan_type: Option<String>,
}

/// Persisted metadata for a saved account slot (`<slot>/account.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountMeta {
    pub schema_version: u32,
    pub agent: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verified_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Stable identity key (schema v2). Never contains a raw token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_key: Option<String>,
}

/// One saved account slot: its metadata, on-disk directory (also usable as the
/// agent's profile home), and whether it matches the live credential.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountSlot {
    pub meta: AccountMeta,
    pub dir: PathBuf,
    pub active: bool,
}

// ---------------------------------------------------------------------------
// Home directories
// ---------------------------------------------------------------------------

fn user_home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}

/// `$CODEX_HOME` if set, otherwise `~/.codex`.
pub fn codex_home() -> Option<PathBuf> {
    if let Some(c) = std::env::var_os("CODEX_HOME") {
        if !c.is_empty() {
            return Some(PathBuf::from(c));
        }
    }
    user_home().map(|h| h.join(".codex"))
}

/// `$CLAUDE_CONFIG_DIR` if set, otherwise `~/.claude`.
pub fn claude_home() -> Option<PathBuf> {
    if let Some(c) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        if !c.is_empty() {
            return Some(PathBuf::from(c));
        }
    }
    user_home().map(|h| h.join(".claude"))
}

fn claude_config_json_path() -> Option<PathBuf> {
    if let Some(c) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        if !c.is_empty() {
            return Some(PathBuf::from(c).join(".claude.json"));
        }
    }
    user_home().map(|h| h.join(".claude.json"))
}

/// Resolve a CLI program on `PATH`, honoring Windows `PATHEXT` so `.cmd`/`.bat`
/// shims (e.g. npm-installed `codex`/`claude`) are found — `std::process` only
/// appends `.exe` by itself. Returns the bare name's full path, or `None`.
pub fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let direct = dir.join(program);
        if direct.is_file() {
            return Some(direct);
        }
        if cfg!(windows) {
            let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into());
            for ext in exts.split(';').filter(|e| !e.is_empty()) {
                let cand = dir.join(format!("{program}{}", ext.to_ascii_lowercase()));
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

/// The live auth file an agent reads on startup.
fn live_auth_path(agent: Agent) -> Option<PathBuf> {
    match agent {
        Agent::Codex => codex_home().map(|h| h.join("auth.json")),
        Agent::Claude => claude_home().map(|h| h.join(".credentials.json")),
    }
}

fn read_live_auth(agent: Agent) -> std::io::Result<Vec<u8>> {
    let live = live_auth_path(agent).ok_or_else(|| std::io::Error::other("no home dir"))?;
    match std::fs::read(&live) {
        Ok(bytes) => Ok(bytes),
        // macOS Claude: the live credential may only exist as a Keychain item.
        #[cfg(target_os = "macos")]
        Err(_) if agent == Agent::Claude && crate::keychain::claude_item_exists() => {
            crate::keychain::read_claude_credentials()
        }
        Err(error) => Err(error),
    }
}

fn live_credential_bytes(agent: Agent) -> Option<Vec<u8>> {
    read_live_auth(agent).ok()
}

// ---------------------------------------------------------------------------
// Codex usage (local rollout files)
// ---------------------------------------------------------------------------

/// Read the most recent Codex `rate_limits` snapshot from the rollout logs.
pub fn codex_status() -> Option<AccountStatus> {
    let dirs: Vec<PathBuf> = codex_home()
        .map(|c| vec![c.join("sessions"), c.join("archived_sessions")])
        .unwrap_or_default();
    let mut files = Vec::new();
    for dir in &dirs {
        collect_jsonl(dir, &mut files);
    }
    // Newest first, so the first rollout carrying rate_limits wins.
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    for (path, _) in files {
        if let Some(status) = last_rate_limits(&path) {
            return Some(status);
        }
    }
    None
}

/// Parse the last `payload.rate_limits` line in a rollout file into a status.
fn last_rate_limits(path: &Path) -> Option<AccountStatus> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .rev()
        .filter(|l| l.contains("\"rate_limits\""))
        .find_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            parse_rate_limits(&value)
        })
}

/// Extract an [`AccountStatus`] from a rollout record's `payload.rate_limits`.
fn parse_rate_limits(record: &Value) -> Option<AccountStatus> {
    let rl = record.get("payload")?.get("rate_limits")?;
    let window = |o: &Value| -> Option<RateWindow> {
        let used_percent = o.get("used_percent")?.as_f64()?;
        let window_minutes = o.get("window_minutes").and_then(Value::as_u64).unwrap_or(0);
        let resets_at = o.get("resets_at").and_then(Value::as_i64);
        Some(RateWindow {
            used_percent,
            window_minutes,
            resets_at,
        })
    };
    let captured_at = record
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|dt| dt.timestamp_millis());
    Some(AccountStatus {
        plan_type: rl
            .get("plan_type")
            .and_then(Value::as_str)
            .map(String::from),
        five_hour: rl.get("primary").and_then(&window),
        weekly: rl.get("secondary").and_then(&window),
        captured_at,
    })
}

// ---------------------------------------------------------------------------
// Codex identity (auth.json JWT — decoded locally, secret never returned)
// ---------------------------------------------------------------------------

/// Decode the (unverified) claims of a JWT's payload segment.
fn decode_jwt_claims(jwt: &str) -> Option<Value> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim())
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Pull display-only identity (email / plan / account id) from `auth.json`.
pub fn codex_identity() -> Option<Identity> {
    let path = codex_home()?.join("auth.json");
    let value: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    identity_from_auth(&value)
}

/// Pure half of [`codex_identity`] (split out so it is unit-testable).
fn identity_from_auth(value: &Value) -> Option<Identity> {
    let tokens = value.get("tokens")?;
    let claims = tokens
        .get("id_token")
        .and_then(Value::as_str)
        .and_then(decode_jwt_claims)
        .unwrap_or(Value::Null);
    let auth_ns = claims.get("https://api.openai.com/auth");
    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .or_else(|| {
            claims
                .get("https://api.openai.com/profile")
                .and_then(|p| p.get("email"))
                .and_then(Value::as_str)
        })
        .map(String::from);
    let account_id = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            auth_ns
                .and_then(|a| a.get("chatgpt_account_id"))
                .and_then(Value::as_str)
                .map(String::from)
        })
        .or_else(|| auth_ns.and_then(default_organization_id));
    let plan_type = auth_ns
        .and_then(|a| a.get("chatgpt_plan_type"))
        .and_then(Value::as_str)
        .map(String::from);
    Some(Identity {
        email,
        account_id,
        plan_type,
    })
}

/// The `(access_token, account_id)` stored in a saved Codex slot.
pub fn slot_request_auth(agent: Agent, label: &str) -> Option<(String, Option<String>)> {
    request_auth_from_path(&slot_dir(agent, label).join(cred_filename(agent)))
}

/// The `(access_token, plan)` stored in a saved Claude slot.
pub fn claude_slot_oauth(label: &str) -> Option<(String, Option<String>)> {
    claude_oauth_from_path(&slot_dir(Agent::Claude, label).join(cred_filename(Agent::Claude)))
}

/// The Claude slot token for a usage fetch, handling snapshot staleness:
/// Claude Code rotates its OAuth access token every few hours, so a saved
/// copy goes stale even though it is still the same account.
///
/// 1. If the slot holds the active account and the live credential is
///    fresher, re-sync the snapshot from the live file first (same identity
///    only — and this runs only from explicit, user-initiated refresh paths).
/// 2. If the stored token is already expired, fail fast with an actionable
///    error instead of a doomed network call and a misleading "re-add".
pub fn claude_slot_fetch_token(label: &str) -> Result<(String, Option<String>), String> {
    sync_active_claude_slot(label);
    let path = slot_dir(Agent::Claude, label).join(cred_filename(Agent::Claude));
    let bytes = std::fs::read(&path).map_err(|_| "no usable token in this account".to_string())?;
    if let Some(expires_at) = claude_expires_at(&bytes) {
        // 60s margin so a token expiring mid-request already counts as stale.
        if expires_at <= chrono::Utc::now().timestamp_millis() + 60_000 {
            return Err(
                "token expired; launch this account (l) or open Claude Code to refresh it"
                    .to_string(),
            );
        }
    }
    claude_oauth_from_bytes(&bytes).ok_or_else(|| "no usable token in this account".to_string())
}

/// Unix-ms expiry recorded in a Claude credential file, if present.
fn claude_expires_at(bytes: &[u8]) -> Option<i64> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let oauth = value.get("claudeAiOauth").or_else(|| value.get("oauth"))?;
    oauth
        .get("expiresAt")
        .or_else(|| oauth.get("expires_at"))
        .and_then(Value::as_i64)
}

/// Re-sync a saved Claude slot from the live credential when both hold the
/// same account (identity key match) and the live token is fresher. Never
/// copies a different account's credential, and never replaces a fresher
/// snapshot with a staler token.
fn sync_active_claude_slot(label: &str) {
    let dir = slot_dir(Agent::Claude, label);
    let cred_path = dir.join(cred_filename(Agent::Claude));
    let Ok(slot_bytes) = std::fs::read(&cred_path) else {
        return;
    };
    let Some(live_bytes) = live_credential_bytes(Agent::Claude) else {
        return;
    };
    if live_bytes == slot_bytes {
        return;
    }
    let Some(meta) = read_meta(&dir) else {
        return;
    };
    let live_identity = claude_identity();
    if identity_key(Agent::Claude, live_identity.as_ref(), &live_bytes)
        != slot_identity_key(Agent::Claude, &meta, &slot_bytes)
    {
        return;
    }
    // Only move forward in time.
    match (
        claude_expires_at(&live_bytes),
        claude_expires_at(&slot_bytes),
    ) {
        (Some(live_exp), Some(slot_exp)) if live_exp > slot_exp => {}
        (Some(_), None) => {}
        _ => return,
    }
    let _ = private_write(&cred_path, &live_bytes);
}

/// Read `(access_token, account_id)` from a Codex `auth.json` at `path`.
fn request_auth_from_path(path: &Path) -> Option<(String, Option<String>)> {
    let value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let access_token = value
        .get("tokens")?
        .get("access_token")?
        .as_str()?
        .to_string();
    let account_id = identity_from_auth(&value).and_then(|i| i.account_id);
    Some((access_token, account_id))
}

fn claude_oauth_from_path(path: &Path) -> Option<(String, Option<String>)> {
    claude_oauth_from_bytes(&std::fs::read(path).ok()?)
}

fn claude_oauth_from_bytes(bytes: &[u8]) -> Option<(String, Option<String>)> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let oauth = value.get("claudeAiOauth").or_else(|| value.get("oauth"))?;
    let access_token = oauth
        .get("accessToken")
        .or_else(|| oauth.get("access_token"))
        .or_else(|| oauth.get("oauth_access_token"))?
        .as_str()?
        .to_string();
    let plan = oauth
        .get("subscriptionType")
        .or_else(|| oauth.get("subscription_type"))
        .and_then(Value::as_str)
        .map(String::from);
    Some((access_token, plan))
}

// ---------------------------------------------------------------------------
// Claude token refresh support (the network exchange lives in `account_api`;
// only local file access lives here so this module stays network-free).
//
// A saved slot is a point-in-time snapshot of `.credentials.json`. Claude
// rotates its short-lived OAuth access token every few hours, so a slot that is
// NOT the active login (and so is never re-synced from the live file) goes stale
// within a day or two and its usage fetch errors out. The stored refresh token
// exchanges for a new access token; these helpers give `account_api` the pieces
// it needs and persist the result back into the slot.
// ---------------------------------------------------------------------------

/// The slot's Claude credential bytes after a best-effort active-slot re-sync
/// from the fresher live login (same identity only). No network.
pub fn claude_slot_synced_bytes(label: &str) -> Option<Vec<u8>> {
    sync_active_claude_slot(label);
    let path = slot_dir(Agent::Claude, label).join(cred_filename(Agent::Claude));
    std::fs::read(&path).ok()
}

/// Unix-ms access-token expiry recorded in a Claude credential, if present.
pub fn claude_credential_expires_at(bytes: &[u8]) -> Option<i64> {
    claude_expires_at(bytes)
}

/// `(access_token, plan)` from a Claude credential, with no expiry gate.
pub fn claude_oauth_pair(bytes: &[u8]) -> Option<(String, Option<String>)> {
    claude_oauth_from_bytes(bytes)
}

/// True when the live Claude credential currently holds exactly `bytes`.
/// The active slot is kept byte-identical to the live login by
/// [`sync_active_claude_slot`], so this identifies "this slot IS the live
/// login" without any identity guesswork.
pub fn claude_live_credential_equals(bytes: &[u8]) -> bool {
    live_credential_bytes(Agent::Claude).is_some_and(|live| live == bytes)
}

/// Copy a slot's (just-refreshed) Claude credential over the live login, but
/// only if the live credential still holds exactly `expected` — the bytes the
/// refresh started from. Needed because the OAuth refresh grant ROTATES the
/// refresh token: after refreshing the active slot, the live file still holds
/// the now-invalidated old refresh token, and Claude Code would be forced into
/// a browser re-login on its next refresh unless it gets the new one.
/// A diverged live credential (Claude Code rotated it itself meanwhile, or the
/// user switched accounts) is never overwritten.
pub fn propagate_claude_slot_to_live(label: &str, expected: &[u8]) -> std::io::Result<()> {
    let current = live_credential_bytes(Agent::Claude)
        .ok_or_else(|| std::io::Error::other("no live credential"))?;
    if current != expected {
        return Err(std::io::Error::other(
            "live credential changed since the refresh started",
        ));
    }
    let bytes = std::fs::read(slot_dir(Agent::Claude, label).join(cred_filename(Agent::Claude)))?;
    // macOS Claude may keep the live credential in the Keychain, not the file.
    #[cfg(target_os = "macos")]
    if live_auth_path(Agent::Claude)
        .map(|p| !p.exists())
        .unwrap_or(false)
        && crate::keychain::claude_item_exists()
    {
        return crate::keychain::write_claude_credentials(&bytes);
    }
    let live = live_auth_path(Agent::Claude).ok_or_else(|| std::io::Error::other("no home dir"))?;
    let tmp = live.with_extension("tmp");
    crate::secure_fs::write_private_atomic_file(&live, &tmp, &bytes)
}

/// The stored OAuth refresh token, if any (used to exchange for a fresh access
/// token when the saved one has expired).
pub fn claude_credential_refresh_token(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let oauth = value.get("claudeAiOauth").or_else(|| value.get("oauth"))?;
    oauth
        .get("refreshToken")
        .or_else(|| oauth.get("refresh_token"))
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(String::from)
}

/// Merge a refreshed token set into the slot's Claude credential and persist it
/// atomically with private permissions, preserving every other field. Returns
/// the new `(access_token, plan)`. Only rewrites the saved slot snapshot — the
/// live login file is Claude Code's to manage.
pub fn persist_claude_slot_refresh(
    label: &str,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_at_ms: i64,
) -> Result<(String, Option<String>), String> {
    let path = slot_dir(Agent::Claude, label).join(cred_filename(Agent::Claude));
    let bytes = std::fs::read(&path).map_err(|_| "no usable token in this account".to_string())?;
    let mut value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "credential is not valid JSON".to_string())?;
    let oauth_key = if value.get("claudeAiOauth").is_some() {
        "claudeAiOauth"
    } else if value.get("oauth").is_some() {
        "oauth"
    } else {
        return Err("credential has no oauth block".to_string());
    };
    let oauth = value
        .get_mut(oauth_key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "credential oauth block is malformed".to_string())?;
    oauth.insert(
        "accessToken".to_string(),
        Value::String(access_token.to_string()),
    );
    if let Some(token) = refresh_token {
        oauth.insert("refreshToken".to_string(), Value::String(token.to_string()));
    }
    oauth.insert("expiresAt".to_string(), Value::Number(expires_at_ms.into()));
    let serialized =
        serde_json::to_vec(&value).map_err(|_| "could not serialize credential".to_string())?;
    private_write(&path, &serialized)
        .map_err(|error| format!("could not persist refreshed token: {error}"))?;
    claude_oauth_from_bytes(&serialized)
        .ok_or_else(|| "no usable token in this account".to_string())
}

fn default_organization_id(auth: &Value) -> Option<String> {
    let orgs = auth.get("organizations")?.as_array()?;
    orgs.iter()
        .find(|org| {
            org.get("is_default")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .or_else(|| orgs.first())
        .and_then(|org| org.get("id").or_else(|| org.get("account_id")))
        .and_then(Value::as_str)
        .map(String::from)
}

// ---------------------------------------------------------------------------
// Claude identity (config, not a credential file)
// ---------------------------------------------------------------------------

/// Pull the Claude account email/plan from `~/.claude.json` (the config — the
/// OAuth tokens live in a separate `.credentials.json` we never read here).
pub fn claude_identity() -> Option<Identity> {
    let path = claude_config_json_path()?;
    let value: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    let acc = value.get("oauthAccount");
    let email = acc
        .and_then(|a| a.get("emailAddress"))
        .and_then(Value::as_str)
        .map(String::from);
    let plan_type = value
        .get("subscriptionType")
        .and_then(Value::as_str)
        .map(String::from);
    if email.is_none() && plan_type.is_none() {
        return None;
    }
    Some(Identity {
        email,
        account_id: None,
        plan_type,
    })
}

// ---------------------------------------------------------------------------
// Claude usage (statusline samples captured by the hook)
// ---------------------------------------------------------------------------

/// The latest Claude usage sample (recorded by the statusline hook).
pub fn claude_status() -> Option<AccountStatus> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    // Accept samples up to a day old so the tab still shows the last reading.
    let usage = crate::sensor::read_claude_rate_limit(24 * 60 * 60 * 1000, now_ms)?;
    let weekly = usage.weekly.map(|w| RateWindow {
        used_percent: w.used_percent,
        window_minutes: w.window_minutes as u64,
        resets_at: w.resets_at.map(|r| r as i64),
    });
    Some(AccountStatus {
        plan_type: None,
        five_hour: Some(RateWindow {
            used_percent: usage.used_percent,
            window_minutes: usage.window_minutes as u64,
            resets_at: usage.resets_at.map(|r| r as i64),
        }),
        weekly,
        captured_at: Some(usage.captured_at),
    })
}

// ---------------------------------------------------------------------------
// Display-gated usage
// ---------------------------------------------------------------------------

/// True when this agent has a saved slot matching the current live credential.
pub fn has_active_slot(agent: Agent) -> bool {
    list_slots(agent).iter().any(|slot| slot.active)
}

/// Local usage status for display surfaces only.
///
/// Ambient CLI rollout/statusline samples belong to the current machine login,
/// not necessarily to an account the user added to ai-handoff. Withhold them
/// until a saved slot matches the live credential. Daemon handoff triggers keep
/// using the raw status readers because they are intentionally live-session
/// scoped.
pub fn display_status(agent: Agent) -> Option<AccountStatus> {
    if !has_active_slot(agent) {
        return None;
    }
    match agent {
        Agent::Codex => codex_status(),
        Agent::Claude => claude_status(),
    }
}
// ---------------------------------------------------------------------------
// Credential vault (per-account slot dirs: metadata + credential)
//
// Layout: <AI_HANDOFF_HOME>/accounts/<agent>/<label>/{account.json, <cred>}
// where <cred> is `auth.json` (Codex) or `.credentials.json` (Claude). The slot
// dir doubles as the agent's profile home (`CODEX_HOME` / `CLAUDE_CONFIG_DIR`)
// for the launch-profile mode.
// ---------------------------------------------------------------------------

/// The live credential file name for an agent (what the agent reads on startup).
fn cred_filename(agent: Agent) -> &'static str {
    match agent {
        Agent::Codex => "auth.json",
        Agent::Claude => ".credentials.json",
    }
}

fn accounts_root(agent: Agent) -> PathBuf {
    crate::paths::home().join("accounts").join(agent.dir())
}

/// The directory of one saved slot (also usable as the agent's profile home).
pub fn slot_dir(agent: Agent, label: &str) -> PathBuf {
    accounts_root(agent).join(sanitize(label))
}

/// The `(env-var, value)` for launching the agent under a slot's profile home.
pub fn profile_env(agent: Agent, label: &str) -> (&'static str, PathBuf) {
    let var = match agent {
        Agent::Codex => "CODEX_HOME",
        Agent::Claude => "CLAUDE_CONFIG_DIR",
    };
    (var, slot_dir(agent, label))
}

/// Sanitize a label into a safe directory name (keeps `@ . _ -` and alnum).
fn sanitize(label: &str) -> String {
    let s: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() {
        "account".to_string()
    } else {
        s
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn read_meta(dir: &Path) -> Option<AccountMeta> {
    serde_json::from_slice(&std::fs::read(dir.join("account.json")).ok()?).ok()
}

/// Stable identity for a credential, independent of the display label.
/// Team/business credentials can share an org/account id across multiple
/// emails, while one email can also have multiple plans. Keep both boundaries.
/// The hash fallback is the first 12 hex chars of SHA-256.
fn identity_key(agent: Agent, identity: Option<&Identity>, cred_bytes: &[u8]) -> String {
    let hash_key = || {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(cred_bytes);
        let hex = format!("{:x}", h.finalize());
        format!("{}:token:{}", agent.dir(), &hex[..12])
    };
    let lower = |s: &str| s.to_ascii_lowercase();
    let email = identity.and_then(|i| i.email.as_deref()).map(lower);
    let account_id = identity.and_then(|i| i.account_id.as_deref()).map(lower);
    let plan = identity.and_then(|i| i.plan_type.as_deref()).map(lower);
    let suffix = |name: &str, value: Option<&String>| {
        value
            .map(|value| format!(":{name}:{value}"))
            .unwrap_or_default()
    };
    match agent {
        Agent::Claude => {
            let org = claude_org_from_cred_bytes(cred_bytes).map(|org| org.to_ascii_lowercase());
            match (email.as_ref(), org.as_ref()) {
                (Some(email), Some(org)) => {
                    format!(
                        "claude:email:{email}:org:{org}{}",
                        suffix("plan", plan.as_ref())
                    )
                }
                (Some(email), None) => {
                    format!("claude:email:{email}{}", suffix("plan", plan.as_ref()))
                }
                (None, Some(org)) => format!("claude:org:{org}{}", suffix("plan", plan.as_ref())),
                (None, None) => hash_key(),
            }
        }
        Agent::Codex => match (email.as_ref(), account_id.as_ref()) {
            (Some(email), Some(account)) => {
                format!(
                    "codex:email:{email}:account:{account}{}",
                    suffix("plan", plan.as_ref())
                )
            }
            (Some(email), None) => format!("codex:email:{email}{}", suffix("plan", plan.as_ref())),
            (None, Some(account)) => {
                format!("codex:account:{account}{}", suffix("plan", plan.as_ref()))
            }
            (None, None) => hash_key(),
        },
    }
}

fn claude_org_from_cred_bytes(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let oauth = value.get("claudeAiOauth").or_else(|| value.get("oauth"))?;
    let token = oauth
        .get("accessToken")
        .or_else(|| oauth.get("access_token"))
        .or_else(|| oauth.get("oauth_access_token"))?
        .as_str()?;
    claude_org_uuid_from_access_token(token)
}

/// Decode the org UUID claim from a Claude OAuth access token locally.
pub fn claude_org_uuid_from_access_token(access_token: &str) -> Option<String> {
    let claims = decode_jwt_claims(access_token)?;
    let org = claims
        .get("organizationUUID")
        .or_else(|| claims.get("organization_uuid"))
        .or_else(|| claims.get("org_uuid"))
        .or_else(|| claims.get("orgUuid"))
        .or_else(|| claims.get("lastActiveOrg"))
        .and_then(Value::as_str)?;
    let safe = !org.is_empty()
        && org
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
    safe.then(|| org.to_string())
}

fn slot_identity_key(agent: Agent, meta: &AccountMeta, cred_bytes: &[u8]) -> String {
    let identity = Identity {
        email: meta.email.clone(),
        account_id: meta.account_id.clone(),
        plan_type: meta.plan_hint.clone(),
    };
    let has_identity_fields =
        identity.email.is_some() || identity.account_id.is_some() || identity.plan_type.is_some();
    if let Some(key) = meta.identity_key.clone() {
        if has_identity_fields {
            return identity_key(agent, Some(&identity), cred_bytes);
        }
        return key;
    }
    identity_key(agent, Some(&identity), cred_bytes)
}

fn read_slot_record(
    agent: Agent,
    dir: PathBuf,
    live: Option<&[u8]>,
    live_key: Option<&str>,
) -> Option<(AccountSlot, String)> {
    if !dir.is_dir() {
        return None;
    }
    let cred = std::fs::read(dir.join(cred_filename(agent))).ok()?;
    let label = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string();
    let meta = read_meta(&dir).unwrap_or(AccountMeta {
        schema_version: 1,
        agent: agent.dir().to_string(),
        label: label.clone(),
        email: None,
        plan_hint: None,
        account_id: None,
        workspace_id: None,
        created_at: None,
        last_verified_at: None,
        source: None,
        identity_key: None,
    });
    let key = slot_identity_key(agent, &meta, &cred);
    let active = live.is_some_and(|l| l == cred.as_slice())
        || live_key.is_some_and(|live_key| live_key == key.as_str());
    Some((AccountSlot { meta, dir, active }, key))
}

fn slot_rank(slot: &AccountSlot) -> (u8, u32, String, String, String) {
    (
        u8::from(slot.active),
        slot.meta.schema_version,
        slot.meta.last_verified_at.clone().unwrap_or_default(),
        slot.meta.created_at.clone().unwrap_or_default(),
        slot.meta.label.clone(),
    )
}

fn prefer_slot(candidate: &AccountSlot, current: &AccountSlot) -> bool {
    slot_rank(candidate) > slot_rank(current)
}

fn upsert_unique_slot(slots: &mut Vec<(AccountSlot, String)>, slot: AccountSlot, key: String) {
    if let Some((current, _)) = slots.iter_mut().find(|(_, existing)| *existing == key) {
        if prefer_slot(&slot, current) {
            *current = slot;
        }
    } else {
        slots.push((slot, key));
    }
}

/// List saved account slots, marking which one matches the live credential.
pub fn list_slots(agent: Agent) -> Vec<AccountSlot> {
    let root = accounts_root(agent);
    let live = live_credential_bytes(agent);
    let live_key = live.as_ref().map(|bytes| {
        let identity = match agent {
            Agent::Codex => codex_identity(),
            Agent::Claude => claude_identity(),
        };
        identity_key(agent, identity.as_ref(), bytes)
    });
    let mut slots = Vec::<(AccountSlot, String)>::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if let Some((slot, key)) =
            read_slot_record(agent, dir, live.as_deref(), live_key.as_deref())
        {
            upsert_unique_slot(&mut slots, slot, key);
        }
    }
    let mut slots = slots.into_iter().map(|(slot, _)| slot).collect::<Vec<_>>();
    slots.sort_by(|a, b| a.meta.label.cmp(&b.meta.label));
    slots
}

/// Capture the agent's current live credential into a new slot (with metadata).
/// Returns the slot label.
pub fn snapshot_current(agent: Agent) -> std::io::Result<String> {
    let bytes = read_live_auth(agent)?;
    let identity = match agent {
        Agent::Codex => codex_identity(),
        Agent::Claude => claude_identity(),
    };
    save_slot(agent, &bytes, identity.as_ref(), "capture-current")
}

/// Persist credential bytes + identity as a slot dir (`account.json` + cred).
/// Used by `snapshot_current` and by the OAuth-login add flow. Returns the label.
pub fn save_slot(
    agent: Agent,
    cred_bytes: &[u8],
    identity: Option<&Identity>,
    source: &str,
) -> std::io::Result<String> {
    let key = identity_key(agent, identity, cred_bytes);
    let base_label = sanitize(&label_from_identity(agent, identity));
    let label = resolve_slot_label(agent, &base_label, cred_bytes, identity, &key);
    let dir = slot_dir(agent, &label);
    // The vault holds raw OAuth tokens: harden the tree and write the
    // credential private+atomic, matching how capsules are already written.
    crate::secure_fs::ensure_private_dir(&accounts_root(agent))?;
    crate::secure_fs::ensure_private_dir(&dir)?;
    private_write(&dir.join(cred_filename(agent)), cred_bytes)?;
    let now = now_rfc3339();
    let meta = AccountMeta {
        schema_version: 2,
        agent: agent.dir().to_string(),
        label: label.clone(),
        email: identity.and_then(|i| i.email.clone()),
        plan_hint: identity.and_then(|i| i.plan_type.clone()),
        account_id: identity.and_then(|i| i.account_id.clone()),
        workspace_id: None,
        created_at: Some(now.clone()),
        last_verified_at: Some(now),
        source: Some(source.to_string()),
        identity_key: Some(key),
    };
    let json = serde_json::to_vec_pretty(&meta).map_err(std::io::Error::other)?;
    private_write(&dir.join("account.json"), &json)?;
    Ok(label)
}

fn resolve_slot_label(
    agent: Agent,
    base_label: &str,
    cred_bytes: &[u8],
    identity: Option<&Identity>,
    key: &str,
) -> String {
    if let Some(label) = existing_slot_label_for_identity(agent, key) {
        return label;
    }

    if slot_can_be_reused(agent, base_label, cred_bytes, key) {
        return base_label.to_string();
    }

    if let Some(email) = identity.and_then(|i| i.email.as_deref()) {
        let email_label = sanitize(email);
        if email_label != base_label && slot_can_be_reused(agent, &email_label, cred_bytes, key) {
            return email_label;
        }
    }

    for n in 2.. {
        let label = format!("{base_label}-{n}");
        if slot_can_be_reused(agent, &label, cred_bytes, key) {
            return label;
        }
    }
    unreachable!("unbounded suffix search must find a slot label")
}

fn existing_slot_label_for_identity(agent: Agent, key: &str) -> Option<String> {
    let root = accounts_root(agent);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return None;
    };
    let mut best: Option<AccountSlot> = None;
    for entry in entries.flatten() {
        let Some((slot, slot_key)) = read_slot_record(agent, entry.path(), None, None) else {
            continue;
        };
        if slot_key != key {
            continue;
        }
        match best.as_ref() {
            Some(current) if !prefer_slot(&slot, current) => {}
            _ => best = Some(slot),
        }
    }
    best.map(|slot| slot.meta.label)
}

fn slot_can_be_reused(agent: Agent, label: &str, cred_bytes: &[u8], key: &str) -> bool {
    let dir = slot_dir(agent, label);
    if !dir.exists() {
        return true;
    }
    let Ok(existing) = std::fs::read(dir.join(cred_filename(agent))) else {
        return true;
    };
    if existing == cred_bytes {
        return true;
    }
    read_meta(&dir)
        .as_ref()
        .is_some_and(|meta| slot_identity_key(agent, meta, &existing) == key)
}

fn label_from_identity(agent: Agent, identity: Option<&Identity>) -> String {
    identity
        .and_then(|i| match agent {
            Agent::Codex => i.account_id.clone().or_else(|| i.email.clone()),
            Agent::Claude => i.email.clone().or_else(|| i.account_id.clone()),
        })
        .unwrap_or_else(|| format!("{}-account", agent.dir()))
}

/// After an official `codex login` / `claude auth login` wrote credentials into
/// `profile_home` (a temp `CODEX_HOME` / `CLAUDE_CONFIG_DIR`), capture them into
/// a vault slot with identity metadata. Returns the slot label.
///
/// The credential bytes never leave this process; only the slot files are
/// written under the accounts vault.
pub fn capture_login(agent: Agent, profile_home: &Path, source: &str) -> std::io::Result<String> {
    let bytes = std::fs::read(profile_home.join(cred_filename(agent))).map_err(|_| {
        std::io::Error::other(
            "no credential file was written (the login may have used the OS keyring)",
        )
    })?;
    let identity = match agent {
        Agent::Codex => serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| identity_from_auth(&v)),
        Agent::Claude => claude_identity_from_dir(profile_home),
    };
    save_slot(agent, &bytes, identity.as_ref(), source)
}

/// Whether an official login into `profile_home` has finished writing a usable
/// credential (used to poll while the vendor CLI runs in another window).
pub fn login_complete(agent: Agent, profile_home: &Path) -> bool {
    let bytes = match std::fs::read(profile_home.join(cred_filename(agent))) {
        Ok(b) if !b.is_empty() => b,
        _ => return false,
    };
    match agent {
        // Codex writes auth.json atomically on success — require a real token.
        Agent::Codex => serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| {
                v.get("tokens")
                    .and_then(|t| t.get("access_token"))
                    .and_then(Value::as_str)
                    .map(|s| !s.is_empty())
            })
            .unwrap_or(false),
        Agent::Claude => claude_oauth_from_path(&profile_home.join(cred_filename(agent)))
            .map(|(token, _)| !token.is_empty())
            .unwrap_or(false),
    }
}

/// Read the Claude account email/plan from a config dir's `.claude.json`.
fn claude_identity_from_dir(dir: &Path) -> Option<Identity> {
    let value: Value =
        serde_json::from_slice(&std::fs::read(dir.join(".claude.json")).ok()?).ok()?;
    let email = value
        .get("oauthAccount")
        .and_then(|a| a.get("emailAddress"))
        .and_then(Value::as_str)
        .map(String::from);
    let plan_type = value
        .get("subscriptionType")
        .and_then(Value::as_str)
        .map(String::from);
    if email.is_none() && plan_type.is_none() {
        return None;
    }
    Some(Identity {
        email,
        account_id: None,
        plan_type,
    })
}

/// Make a saved slot the live credential (atomic file swap). For Claude, also
/// surgically update `~/.claude.json` `oauthAccount` so the shown account
/// matches — the rest of that large shared config is left intact.
pub fn switch_slot(agent: Agent, label: &str) -> std::io::Result<()> {
    // macOS Claude keeps its token in the Keychain, not the file — a file swap
    // would not change the live login. Swap the Keychain item instead when one
    // exists (the real login); with neither a file nor an item, fail honestly.
    #[cfg(target_os = "macos")]
    if agent == Agent::Claude && live_auth_path(agent).map(|p| !p.exists()).unwrap_or(false) {
        if crate::keychain::claude_item_exists() {
            let dir = slot_dir(agent, label);
            let bytes = std::fs::read(dir.join(cred_filename(agent)))?;
            crate::keychain::write_claude_credentials(&bytes)?;
            let _ = patch_claude_oauth_account(read_meta(&dir).and_then(|m| m.email));
            return Ok(());
        }
        return Err(std::io::Error::other(
            "no live Claude credential found (no credential file and no Keychain item) — sign in with Claude Code once, then switch",
        ));
    }
    let dir = slot_dir(agent, label);
    let bytes = std::fs::read(dir.join(cred_filename(agent)))?;
    let live = live_auth_path(agent).ok_or_else(|| std::io::Error::other("no home dir"))?;
    // Private-file write: a plain fs::write would recreate the credential with
    // the umask default (0644 on most unix), silently downgrading the 0600 the
    // agent itself uses. Only the file is hardened — never the agent's home
    // dir, whose ACL the agent's sandbox may depend on.
    let tmp = live.with_extension("tmp");
    crate::secure_fs::write_private_atomic_file(&live, &tmp, &bytes)?;
    if agent == Agent::Claude {
        let _ = patch_claude_oauth_account(read_meta(&dir).and_then(|m| m.email));
    }
    Ok(())
}

/// Best-effort: set `oauthAccount.emailAddress` in `~/.claude.json` without
/// replacing the file (it holds projects/history/settings too).
fn patch_claude_oauth_account(email: Option<String>) -> std::io::Result<()> {
    let Some(email) = email else { return Ok(()) };
    let Some(path) = claude_config_json_path() else {
        return Ok(());
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(());
    };
    let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(());
    };
    let Some(obj) = value.as_object_mut() else {
        return Ok(());
    };
    match obj.get_mut("oauthAccount").and_then(|a| a.as_object_mut()) {
        Some(acc) => {
            acc.insert("emailAddress".into(), Value::String(email));
        }
        None => {
            obj.insert(
                "oauthAccount".into(),
                serde_json::json!({ "emailAddress": email }),
            );
        }
    }
    let json = serde_json::to_vec_pretty(&value).map_err(std::io::Error::other)?;
    atomic_write(&path, &json)
}

// ---------------------------------------------------------------------------
// Running-session detection (warn before a live switch)
// ---------------------------------------------------------------------------

/// Best-effort: is the agent's CLI/app currently running? A live credential
/// switch while a session is open may leave that session on the old account, so
/// the UI warns. Returns `false` if the process list can't be read.
pub fn agent_running(agent: Agent) -> bool {
    let marker = match agent {
        Agent::Codex => "codex",
        Agent::Claude => "claude",
    };
    running_process_names().iter().any(|n| n.contains(marker))
}

fn running_process_names() -> Vec<String> {
    // no_window_command keeps `tasklist` (polled by the limit popup)
    // invisible on Windows.
    #[cfg(windows)]
    let output = crate::process::no_window_command("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output();
    #[cfg(not(windows))]
    let output = crate::process::no_window_command("ps")
        .args(["-A", "-o", "comm="])
        .output();
    match output {
        Ok(o) => parse_process_names(&String::from_utf8_lossy(&o.stdout).to_lowercase()),
        Err(_) => Vec::new(),
    }
}

/// Parse process names from the platform listing. Windows `tasklist` CSV has the
/// image name as the first quoted field; `ps -o comm=` is one name per line.
fn parse_process_names(text: &str) -> Vec<String> {
    if cfg!(windows) {
        text.lines()
            .filter_map(|l| l.split('"').nth(1).map(str::to_string))
            .collect()
    } else {
        text.lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }
}

/// Remove a saved slot (its whole directory).
pub fn delete_slot(agent: Agent, label: &str) -> std::io::Result<()> {
    match std::fs::remove_dir_all(slot_dir(agent, label)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Private (0600 / user-only ACL) atomic write for vault slot files. The tmp
/// file lives in the same (already-hardened) slot directory.
fn private_write(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = target.with_extension("tmp");
    crate::secure_fs::write_private_atomic(target, &tmp, bytes)
}

/// Write `bytes` to `target` atomically (tmp in the same dir, then rename).
/// Only for non-credential files (`.claude.json` config patching); credential
/// writes must go through `private_write` / `write_private_atomic_file`.
fn atomic_write(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = target.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, target) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Shared file walking
// ---------------------------------------------------------------------------

/// Collect `(path, modified)` for every `*.jsonl` under `root` (iterative).
fn collect_jsonl(root: &Path, out: &mut Vec<(PathBuf, std::time::SystemTime)>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let mtime = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                out.push((path, mtime));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64url(s: &str) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.as_bytes())
    }

    /// A JWT with the given claims JSON in the payload (header/sig are dummies).
    fn fake_jwt(claims: &str) -> String {
        format!("{}.{}.{}", b64url("{}"), b64url(claims), "sig")
    }

    #[test]
    fn parse_rate_limits_reads_primary_and_secondary() {
        let line = serde_json::json!({
            "timestamp": "2026-06-26T16:58:48Z",
            "payload": { "rate_limits": {
                "primary": { "used_percent": 100.0, "window_minutes": 300, "resets_at": 1782478701i64 },
                "secondary": { "used_percent": 87.0, "window_minutes": 10080, "resets_at": 1782808275i64 },
                "credits": { "has_credits": false, "unlimited": false, "balance": null },
                "plan_type": "team"
            }}
        });
        let status = parse_rate_limits(&line).expect("status");
        assert_eq!(status.plan_type.as_deref(), Some("team"));
        let five = status.five_hour.expect("5h");
        assert_eq!(five.used_percent, 100.0);
        assert_eq!(five.window_minutes, 300);
        assert_eq!(five.remaining_percent(), 0.0);
        let weekly = status.weekly.expect("weekly");
        assert_eq!(weekly.window_minutes, 10080);
        assert_eq!(weekly.resets_at, Some(1782808275));
        assert!(status.captured_at.is_some());
    }

    #[test]
    fn parse_rate_limits_rejects_unrelated_records() {
        let line = serde_json::json!({ "payload": { "type": "message" } });
        assert!(parse_rate_limits(&line).is_none());
    }

    fn write_codex_rollout(codex_home: &Path) {
        let sessions = codex_home.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("rollout-test.jsonl"),
            serde_json::json!({
                "timestamp": "2026-06-26T16:58:48Z",
                "payload": { "rate_limits": {
                    "primary": { "used_percent": 23.0, "window_minutes": 300 },
                    "secondary": { "used_percent": 50.0, "window_minutes": 10080 },
                    "plan_type": "pro"
                }}
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn display_status_requires_active_slot() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let codex_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CODEX_HOME", codex_home.path());
        write_codex_rollout(codex_home.path());

        assert!(codex_status().is_some(), "raw rollout sample exists");
        assert!(!has_active_slot(Agent::Codex));
        assert!(display_status(Agent::Codex).is_none());

        let live = codex_home.path().join("auth.json");
        let credential =
            br#"{"tokens":{"id_token":"x","account_id":"work","access_token":"secret"}}"#;
        std::fs::write(&live, credential).unwrap();
        save_slot(Agent::Codex, credential, None, "test").unwrap();

        assert!(has_active_slot(Agent::Codex));
        assert_eq!(
            display_status(Agent::Codex)
                .and_then(|status| status.five_hour)
                .map(|window| window.used_percent),
            Some(23.0)
        );

        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CODEX_HOME");
    }
    #[test]
    fn claude_status_maps_statusline_seven_day_to_weekly() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let now_ms = chrono::Utc::now().timestamp_millis();
        let input = serde_json::json!({
            "session_id": "claude-weekly",
            "rate_limits": {
                "five_hour": { "used_percentage": 20.0 },
                "seven_day": {
                    "used_percentage": 45.0,
                    "resets_at": 1_900_000_000.0
                }
            }
        });
        assert!(crate::sensor::record_claude_rate_limit(&input, now_ms));

        let status = claude_status().expect("claude status");
        let weekly = status.weekly.expect("weekly status");
        assert_eq!(weekly.used_percent, 45.0);
        assert_eq!(weekly.window_minutes, 10080);
        assert_eq!(weekly.resets_at, Some(1_900_000_000));

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn claude_slot_oauth_reads_saved_access_token_and_plan() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let dir = slot_dir(Agent::Claude, "dev@example.com");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".credentials.json"),
            br#"{"claudeAiOauth":{"accessToken":"secret-token","subscriptionType":"pro"}}"#,
        )
        .unwrap();

        let (token, plan) = claude_slot_oauth("dev@example.com").expect("oauth token");

        assert_eq!(token, "secret-token");
        assert_eq!(plan.as_deref(), Some("pro"));
        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn claude_slot_fetch_token_fails_fast_when_expired() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", live.path());

        let dir = slot_dir(Agent::Claude, "stale@example.com");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".credentials.json"),
            br#"{"claudeAiOauth":{"accessToken":"stale-token","subscriptionType":"pro","expiresAt":1000}}"#,
        )
        .unwrap();

        let err = claude_slot_fetch_token("stale@example.com").unwrap_err();

        assert!(err.contains("token expired"), "got: {err}");
        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn claude_slot_fetch_token_returns_unexpired_token() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", live.path());

        let future = chrono::Utc::now().timestamp_millis() + 3_600_000;
        let dir = slot_dir(Agent::Claude, "fresh@example.com");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".credentials.json"),
            format!(
                r#"{{"claudeAiOauth":{{"accessToken":"fresh-token","subscriptionType":"pro","expiresAt":{future}}}}}"#
            ),
        )
        .unwrap();

        let (token, plan) = claude_slot_fetch_token("fresh@example.com").expect("token");

        assert_eq!(token, "fresh-token");
        assert_eq!(plan.as_deref(), Some("pro"));
        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn claude_credential_refresh_token_reads_stored_token() {
        let with =
            br#"{"claudeAiOauth":{"accessToken":"a","refreshToken":"rt-123","expiresAt":1000}}"#;
        assert_eq!(
            claude_credential_refresh_token(with).as_deref(),
            Some("rt-123")
        );
        let snake = br#"{"oauth":{"access_token":"a","refresh_token":"rt-9"}}"#;
        assert_eq!(
            claude_credential_refresh_token(snake).as_deref(),
            Some("rt-9")
        );
        let none = br#"{"claudeAiOauth":{"accessToken":"a"}}"#;
        assert!(claude_credential_refresh_token(none).is_none());
        let empty = br#"{"claudeAiOauth":{"accessToken":"a","refreshToken":""}}"#;
        assert!(claude_credential_refresh_token(empty).is_none());
    }

    #[test]
    fn persist_claude_slot_refresh_rewrites_token_and_preserves_other_fields() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let dir = slot_dir(Agent::Claude, "dev@example.com");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".credentials.json"),
            br#"{"claudeAiOauth":{"accessToken":"old","refreshToken":"rt-old","expiresAt":1000,"subscriptionType":"pro","scopes":["a","b"]}}"#,
        )
        .unwrap();

        let (token, plan) = persist_claude_slot_refresh(
            "dev@example.com",
            "new-access",
            Some("rt-new"),
            9_999_999_999_999,
        )
        .expect("persist");

        assert_eq!(token, "new-access");
        assert_eq!(plan.as_deref(), Some("pro"));

        let written: Value =
            serde_json::from_slice(&std::fs::read(dir.join(".credentials.json")).unwrap()).unwrap();
        let oauth = &written["claudeAiOauth"];
        assert_eq!(oauth["accessToken"], "new-access");
        assert_eq!(oauth["refreshToken"], "rt-new");
        assert_eq!(oauth["expiresAt"], 9_999_999_999_999i64);
        // Untouched fields survive the merge.
        assert_eq!(oauth["subscriptionType"], "pro");
        assert_eq!(oauth["scopes"][1], "b");

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn propagate_claude_slot_to_live_updates_byte_identical_live() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", live.path());

        let stale =
            br#"{"claudeAiOauth":{"accessToken":"old","refreshToken":"rt-old","expiresAt":1000}}"#;
        let label = save_slot(Agent::Claude, stale, None, "test").unwrap();
        std::fs::write(live.path().join(".credentials.json"), stale).unwrap();

        // Refresh the slot, then propagate to the (still byte-identical) live.
        persist_claude_slot_refresh(&label, "new-access", Some("rt-new"), 5000).unwrap();
        propagate_claude_slot_to_live(&label, stale).expect("propagate");

        let live_bytes = std::fs::read(live.path().join(".credentials.json")).unwrap();
        assert_eq!(
            live_bytes,
            std::fs::read(slot_dir(Agent::Claude, &label).join(".credentials.json")).unwrap(),
            "live must hold the refreshed slot credential"
        );
        let value: Value = serde_json::from_slice(&live_bytes).unwrap();
        assert_eq!(value["claudeAiOauth"]["refreshToken"], "rt-new");

        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn propagate_claude_slot_to_live_refuses_a_diverged_live() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", live.path());

        let stale =
            br#"{"claudeAiOauth":{"accessToken":"old","refreshToken":"rt-old","expiresAt":1000}}"#;
        let label = save_slot(Agent::Claude, stale, None, "test").unwrap();
        // Live moved on (Claude Code rotated it itself, or another account).
        let diverged = br#"{"claudeAiOauth":{"accessToken":"cc-new","refreshToken":"rt-cc","expiresAt":9999}}"#;
        std::fs::write(live.path().join(".credentials.json"), diverged).unwrap();

        persist_claude_slot_refresh(&label, "new-access", Some("rt-new"), 5000).unwrap();
        propagate_claude_slot_to_live(&label, stale).expect_err("must refuse");

        assert_eq!(
            std::fs::read(live.path().join(".credentials.json")).unwrap(),
            diverged,
            "a diverged live credential must never be overwritten"
        );

        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn persist_claude_slot_refresh_keeps_old_refresh_token_when_none_returned() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let dir = slot_dir(Agent::Claude, "keep@example.com");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".credentials.json"),
            br#"{"claudeAiOauth":{"accessToken":"old","refreshToken":"rt-keep","expiresAt":1000}}"#,
        )
        .unwrap();

        persist_claude_slot_refresh("keep@example.com", "new", None, 5000).expect("persist");

        let written: Value =
            serde_json::from_slice(&std::fs::read(dir.join(".credentials.json")).unwrap()).unwrap();
        assert_eq!(written["claudeAiOauth"]["refreshToken"], "rt-keep");
        assert_eq!(written["claudeAiOauth"]["accessToken"], "new");

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn claude_slot_fetch_token_resyncs_active_slot_from_fresher_live() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", live.path());

        // Saved snapshot: expired token for org-123.
        let stale_token = fake_jwt(r#"{"organizationUUID":"org-123"}"#);
        let stale = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{stale_token}","subscriptionType":"pro","expiresAt":1000}}}}"#
        );
        let label = save_slot(Agent::Claude, stale.as_bytes(), None, "test").unwrap();

        // Live credential: same org, rotated token, fresh expiry.
        let fresh_token = fake_jwt(r#"{"organizationUUID":"org-123","iat":2}"#);
        let future = chrono::Utc::now().timestamp_millis() + 3_600_000;
        let fresh = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{fresh_token}","subscriptionType":"pro","expiresAt":{future}}}}}"#
        );
        std::fs::write(live.path().join(".credentials.json"), fresh.as_bytes()).unwrap();

        let (token, _) = claude_slot_fetch_token(&label).expect("re-synced token");

        assert_eq!(token, fresh_token, "must use the fresher live token");
        assert_eq!(
            std::fs::read(slot_dir(Agent::Claude, &label).join(".credentials.json")).unwrap(),
            fresh.as_bytes(),
            "snapshot must be re-synced on disk"
        );
        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn claude_slot_fetch_token_never_syncs_a_different_identity() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", live.path());

        let stale_token = fake_jwt(r#"{"organizationUUID":"org-123"}"#);
        let stale =
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{stale_token}","expiresAt":1000}}}}"#);
        let label = save_slot(Agent::Claude, stale.as_bytes(), None, "test").unwrap();

        // Live credential belongs to a *different* org — must never be copied.
        let other_token = fake_jwt(r#"{"organizationUUID":"org-999"}"#);
        let future = chrono::Utc::now().timestamp_millis() + 3_600_000;
        let other = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{other_token}","expiresAt":{future}}}}}"#
        );
        std::fs::write(live.path().join(".credentials.json"), other.as_bytes()).unwrap();

        let err = claude_slot_fetch_token(&label).unwrap_err();

        assert!(err.contains("token expired"), "got: {err}");
        assert_eq!(
            std::fs::read(slot_dir(Agent::Claude, &label).join(".credentials.json")).unwrap(),
            stale.as_bytes(),
            "a different account's live credential must never overwrite the slot"
        );
        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn claude_login_complete_requires_usable_oauth_token() {
        let dir = tempfile::tempdir().unwrap();
        let cred = dir.path().join(".credentials.json");

        std::fs::write(&cred, b"{}").unwrap();
        assert!(
            !login_complete(Agent::Claude, dir.path()),
            "empty Claude credentials must not be captured"
        );

        std::fs::write(
            &cred,
            br#"{"claudeAiOauth":{"accessToken":"secret-token","subscriptionType":"pro"}}"#,
        )
        .unwrap();
        assert!(login_complete(Agent::Claude, dir.path()));
    }

    #[test]
    fn capture_login_saves_slot_without_switching_live() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", live.path());

        let credential =
            br#"{"claudeAiOauth":{"accessToken":"secret-token","subscriptionType":"pro"}}"#;
        std::fs::write(profile.path().join(".credentials.json"), credential).unwrap();
        std::fs::write(
            profile.path().join(".claude.json"),
            br#"{"oauthAccount":{"emailAddress":"dev@example.com"},"subscriptionType":"pro"}"#,
        )
        .unwrap();
        std::fs::write(live.path().join(".claude.json"), b"{}").unwrap();

        let label = capture_login(Agent::Claude, profile.path(), "official-cli-login").unwrap();

        assert_eq!(label, "dev@example.com");
        assert!(!live.path().join(".credentials.json").exists());
        let slots = list_slots(Agent::Claude);
        assert_eq!(slots.len(), 1);
        assert!(!slots[0].active);
        assert_eq!(slots[0].meta.email.as_deref(), Some("dev@example.com"));

        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn switch_slot_without_file_or_keychain_item_fails_honestly() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", live.path());
        // Never touch a real developer/CI Keychain from the test suite.
        std::env::set_var("AI_HANDOFF_NO_KEYCHAIN", "1");

        let dir = slot_dir(Agent::Claude, "dev@example.com");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".credentials.json"),
            br#"{"claudeAiOauth":{"accessToken":"secret-token","subscriptionType":"pro"}}"#,
        )
        .unwrap();

        let error = switch_slot(Agent::Claude, "dev@example.com")
            .expect_err("no live file and no Keychain item: the switch must fail");

        assert!(error.to_string().contains("Keychain"));
        assert!(!live.path().join(".credentials.json").exists());

        std::env::remove_var("AI_HANDOFF_NO_KEYCHAIN");
        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn identity_key_prefers_org_then_email_then_hash_for_claude() {
        let token = fake_jwt(r#"{"organizationUUID":"org-123"}"#);
        let cred = format!(r#"{{"claudeAiOauth":{{"accessToken":"{token}"}}}}"#);
        assert_eq!(
            identity_key(Agent::Claude, None, cred.as_bytes()),
            "claude:org:org-123"
        );

        let email_id = Identity {
            email: Some("Dev@Example.com".into()),
            account_id: None,
            plan_type: None,
        };
        let opaque = br#"{"claudeAiOauth":{"accessToken":"opaque-token"}}"#;
        assert_eq!(
            identity_key(Agent::Claude, Some(&email_id), opaque),
            "claude:email:dev@example.com"
        );

        let key = identity_key(Agent::Claude, None, opaque);
        assert!(key.starts_with("claude:token:"));
        assert_eq!(key.len(), "claude:token:".len() + 12);
        assert!(!key.contains("opaque-token"));
    }

    #[test]
    fn claude_org_uuid_rejects_unsafe_segments() {
        let token = fake_jwt(r#"{"organizationUUID":"../secret"}"#);
        assert!(claude_org_uuid_from_access_token(&token).is_none());
    }

    #[test]
    fn save_slot_reuses_slot_after_token_refresh_same_claude_identity() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let identity = Identity {
            email: Some("dev@example.com".into()),
            account_id: None,
            plan_type: Some("pro".into()),
        };
        let old = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{}"}}}}"#,
            fake_jwt(r#"{"organizationUUID":"org-123"}"#)
        );
        let new = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{}"}}}}"#,
            fake_jwt(r#"{"organizationUUID":"org-123","iat":2}"#)
        );

        let first = save_slot(Agent::Claude, old.as_bytes(), Some(&identity), "test").unwrap();
        let second = save_slot(Agent::Claude, new.as_bytes(), Some(&identity), "test").unwrap();

        assert_eq!(first, second);
        assert_eq!(list_slots(Agent::Claude).len(), 1);
        assert_eq!(
            std::fs::read(slot_dir(Agent::Claude, &first).join(".credentials.json")).unwrap(),
            new.as_bytes()
        );

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn save_slot_keeps_claude_same_org_different_email_as_separate_slots() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let dev = Identity {
            email: Some("dev@example.com".into()),
            account_id: None,
            plan_type: Some("pro".into()),
        };
        let ops = Identity {
            email: Some("ops@example.com".into()),
            account_id: None,
            plan_type: Some("pro".into()),
        };
        let dev_cred = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{}","subscriptionType":"pro"}}}}"#,
            fake_jwt(r#"{"organizationUUID":"org-123"}"#)
        );
        let ops_cred = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{}","subscriptionType":"pro"}}}}"#,
            fake_jwt(r#"{"organizationUUID":"org-123","iat":2}"#)
        );

        let first = save_slot(Agent::Claude, dev_cred.as_bytes(), Some(&dev), "test").unwrap();
        let second = save_slot(Agent::Claude, ops_cred.as_bytes(), Some(&ops), "test").unwrap();

        assert_eq!(first, "dev@example.com");
        assert_eq!(second, "ops@example.com");
        let slots = list_slots(Agent::Claude);
        assert_eq!(slots.len(), 2);
        assert!(slots
            .iter()
            .any(|slot| slot.meta.email.as_deref() == Some("dev@example.com")));
        assert!(slots
            .iter()
            .any(|slot| slot.meta.email.as_deref() == Some("ops@example.com")));

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn list_slots_marks_active_by_identity_after_token_refresh() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", live.path());

        let saved = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{}"}}}}"#,
            fake_jwt(r#"{"organizationUUID":"org-123"}"#)
        );
        save_slot(Agent::Claude, saved.as_bytes(), None, "test").unwrap();

        let refreshed = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{}"}}}}"#,
            fake_jwt(r#"{"organizationUUID":"org-123","iat":2}"#)
        );
        std::fs::write(live.path().join(".credentials.json"), refreshed.as_bytes()).unwrap();

        let slots = list_slots(Agent::Claude);
        assert_eq!(slots.len(), 1);
        assert!(slots[0].active);

        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn identity_decodes_email_plan_and_account_from_jwt() {
        let claims = r#"{
            "email": "dev@example.com",
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": "pro",
                "chatgpt_account_id": "acc-123"
            }
        }"#;
        let auth = serde_json::json!({
            "tokens": { "id_token": fake_jwt(claims), "access_token": "secret-xyz" }
        });
        let id = identity_from_auth(&auth).expect("identity");
        assert_eq!(id.email.as_deref(), Some("dev@example.com"));
        assert_eq!(id.plan_type.as_deref(), Some("pro"));
        assert_eq!(id.account_id.as_deref(), Some("acc-123"));
    }

    #[test]
    fn identity_prefers_explicit_account_id_field() {
        let auth = serde_json::json!({
            "tokens": { "id_token": fake_jwt("{\"email\":\"a@b.c\"}"), "account_id": "explicit" }
        });
        let id = identity_from_auth(&auth).expect("identity");
        assert_eq!(id.account_id.as_deref(), Some("explicit"));
    }

    #[test]
    fn identity_uses_default_organization_as_account_id_fallback() {
        let claims = r#"{
            "email": "same@example.com",
            "https://api.openai.com/auth": {
                "organizations": [
                    { "id": "acc-other", "is_default": false },
                    { "id": "acc-default", "is_default": true }
                ]
            }
        }"#;
        let auth = serde_json::json!({
            "tokens": { "id_token": fake_jwt(claims), "access_token": "secret-xyz" }
        });
        let id = identity_from_auth(&auth).expect("identity");
        assert_eq!(id.account_id.as_deref(), Some("acc-default"));
    }

    #[test]
    fn codex_slot_label_prefers_account_id_over_email() {
        let personal = Identity {
            email: Some("same@example.com".into()),
            account_id: Some("acc-personal".into()),
            plan_type: Some("plus".into()),
        };
        let work = Identity {
            email: Some("same@example.com".into()),
            account_id: Some("acc-work".into()),
            plan_type: Some("business".into()),
        };

        assert_eq!(
            label_from_identity(Agent::Codex, Some(&personal)),
            "acc-personal"
        );
        assert_eq!(label_from_identity(Agent::Codex, Some(&work)), "acc-work");
        assert_ne!(
            label_from_identity(Agent::Codex, Some(&personal)),
            label_from_identity(Agent::Codex, Some(&work))
        );
    }

    #[test]
    fn save_slot_keeps_codex_same_account_id_different_email_as_separate_slots() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let naver = Identity {
            email: Some("h2171@naver.com".into()),
            account_id: Some("shared-account".into()),
            plan_type: Some("plus".into()),
        };
        let gmail = Identity {
            email: Some("h2171@gmail.com".into()),
            account_id: Some("shared-account".into()),
            plan_type: Some("plus".into()),
        };

        let first = save_slot(Agent::Codex, b"naver-credential", Some(&naver), "test").unwrap();
        let second = save_slot(Agent::Codex, b"gmail-credential", Some(&gmail), "test").unwrap();

        assert_eq!(first, "shared-account");
        assert_ne!(second, first);
        let slots = list_slots(Agent::Codex);
        assert_eq!(slots.len(), 2);
        assert!(slots
            .iter()
            .any(|slot| slot.meta.email.as_deref() == Some("h2171@naver.com")));
        assert!(slots
            .iter()
            .any(|slot| slot.meta.email.as_deref() == Some("h2171@gmail.com")));
        assert_eq!(
            std::fs::read(slot_dir(Agent::Codex, &second).join("auth.json")).unwrap(),
            b"gmail-credential"
        );

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn save_slot_updates_same_identity_in_place() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let identity = Identity {
            email: Some("same@example.com".into()),
            account_id: Some("same-account".into()),
            plan_type: Some("plus".into()),
        };

        let first = save_slot(Agent::Codex, b"old-token", Some(&identity), "test").unwrap();
        let second = save_slot(Agent::Codex, b"new-token", Some(&identity), "test").unwrap();

        assert_eq!(first, second);
        assert_eq!(list_slots(Agent::Codex).len(), 1);
        assert_eq!(
            std::fs::read(slot_dir(Agent::Codex, &first).join("auth.json")).unwrap(),
            b"new-token"
        );

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn save_slot_reuses_legacy_codex_email_slot_with_same_account_id() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let legacy_dir = slot_dir(Agent::Codex, "zh2171@naver.com");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("auth.json"), b"old-token").unwrap();
        let legacy = AccountMeta {
            schema_version: 1,
            agent: "codex".into(),
            label: "zh2171@naver.com".into(),
            email: Some("zh2171@naver.com".into()),
            plan_hint: Some("team".into()),
            account_id: Some("a4cab892-64cc-47f3-a006-7baab1eb4fe9".into()),
            workspace_id: None,
            created_at: Some("2026-07-02T14:37:07Z".into()),
            last_verified_at: Some("2026-07-02T14:37:07Z".into()),
            source: Some("official-cli-login".into()),
            identity_key: None,
        };
        std::fs::write(
            legacy_dir.join("account.json"),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let identity = Identity {
            email: Some("zh2171@naver.com".into()),
            account_id: Some("a4cab892-64cc-47f3-a006-7baab1eb4fe9".into()),
            plan_type: Some("team".into()),
        };
        let label = save_slot(
            Agent::Codex,
            b"new-token",
            Some(&identity),
            "capture-current",
        )
        .unwrap();

        assert_eq!(label, "zh2171@naver.com");
        assert!(!slot_dir(Agent::Codex, "a4cab892-64cc-47f3-a006-7baab1eb4fe9").exists());
        let slots = list_slots(Agent::Codex);
        assert_eq!(slots.len(), 1);
        assert_eq!(
            slots[0].meta.account_id.as_deref(),
            Some("a4cab892-64cc-47f3-a006-7baab1eb4fe9")
        );
        assert_eq!(
            std::fs::read(slot_dir(Agent::Codex, &label).join("auth.json")).unwrap(),
            b"new-token"
        );

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn list_slots_collapses_duplicate_codex_slots_with_same_account_id() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        for (label, schema_version, source) in [
            ("zh2171@naver.com", 1, "official-cli-login"),
            ("a4cab892-64cc-47f3-a006-7baab1eb4fe9", 2, "capture-current"),
        ] {
            let dir = slot_dir(Agent::Codex, label);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("auth.json"), format!("{label}-token")).unwrap();
            let meta = AccountMeta {
                schema_version,
                agent: "codex".into(),
                label: label.into(),
                email: Some("zh2171@naver.com".into()),
                plan_hint: Some("team".into()),
                account_id: Some("a4cab892-64cc-47f3-a006-7baab1eb4fe9".into()),
                workspace_id: None,
                created_at: Some("2026-07-04T03:33:28Z".into()),
                last_verified_at: Some("2026-07-04T03:33:28Z".into()),
                source: Some(source.into()),
                identity_key: (schema_version == 2)
                    .then(|| "codex:account:a4cab892-64cc-47f3-a006-7baab1eb4fe9".into()),
            };
            std::fs::write(
                dir.join("account.json"),
                serde_json::to_vec_pretty(&meta).unwrap(),
            )
            .unwrap();
        }

        let slots = list_slots(Agent::Codex);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].meta.email.as_deref(), Some("zh2171@naver.com"));
        assert_eq!(
            slots[0].meta.account_id.as_deref(),
            Some("a4cab892-64cc-47f3-a006-7baab1eb4fe9")
        );

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn list_slots_keeps_codex_same_email_with_different_account_ids() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        for (label, account_id, plan) in [
            ("personal-account", "acc-personal", "plus"),
            ("business-account", "acc-business", "team"),
        ] {
            let dir = slot_dir(Agent::Codex, label);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("auth.json"), format!("{label}-token")).unwrap();
            let meta = AccountMeta {
                schema_version: 2,
                agent: "codex".into(),
                label: label.into(),
                email: Some("same@example.com".into()),
                plan_hint: Some(plan.into()),
                account_id: Some(account_id.into()),
                workspace_id: None,
                created_at: Some("2026-07-04T03:33:28Z".into()),
                last_verified_at: Some("2026-07-04T03:33:28Z".into()),
                source: Some("test".into()),
                identity_key: Some(format!("codex:account:{account_id}")),
            };
            std::fs::write(
                dir.join("account.json"),
                serde_json::to_vec_pretty(&meta).unwrap(),
            )
            .unwrap();
        }

        let slots = list_slots(Agent::Codex);
        assert_eq!(slots.len(), 2);
        assert!(slots
            .iter()
            .any(|slot| slot.meta.plan_hint.as_deref() == Some("plus")));
        assert!(slots
            .iter()
            .any(|slot| slot.meta.plan_hint.as_deref() == Some("team")));

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn list_slots_keeps_codex_same_account_id_with_different_plans() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        for (label, plan) in [("personal-account", "plus"), ("business-account", "team")] {
            let dir = slot_dir(Agent::Codex, label);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("auth.json"), format!("{label}-token")).unwrap();
            let meta = AccountMeta {
                schema_version: 2,
                agent: "codex".into(),
                label: label.into(),
                email: Some("same@example.com".into()),
                plan_hint: Some(plan.into()),
                account_id: Some("shared-account".into()),
                workspace_id: None,
                created_at: Some("2026-07-04T03:33:28Z".into()),
                last_verified_at: Some("2026-07-04T03:33:28Z".into()),
                source: Some("test".into()),
                identity_key: Some("codex:account:shared-account".into()),
            };
            std::fs::write(
                dir.join("account.json"),
                serde_json::to_vec_pretty(&meta).unwrap(),
            )
            .unwrap();
        }

        let slots = list_slots(Agent::Codex);
        assert_eq!(slots.len(), 2);
        assert!(slots
            .iter()
            .any(|slot| slot.meta.plan_hint.as_deref() == Some("plus")));
        assert!(slots
            .iter()
            .any(|slot| slot.meta.plan_hint.as_deref() == Some("team")));

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    fn save_slot_keeps_codex_same_account_id_different_plan_as_separate_slot() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let personal = Identity {
            email: Some("same@example.com".into()),
            account_id: Some("shared-account".into()),
            plan_type: Some("plus".into()),
        };
        let business = Identity {
            email: Some("same@example.com".into()),
            account_id: Some("shared-account".into()),
            plan_type: Some("team".into()),
        };

        let first = save_slot(Agent::Codex, b"plus-token", Some(&personal), "test").unwrap();
        let second = save_slot(Agent::Codex, b"team-token", Some(&business), "test").unwrap();

        assert_eq!(first, "shared-account");
        assert_ne!(second, first);
        let slots = list_slots(Agent::Codex);
        assert_eq!(slots.len(), 2);

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    #[cfg(unix)]
    fn save_slot_writes_private_slot_dir_and_files() {
        use std::os::unix::fs::PermissionsExt;
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());

        let label = save_slot(
            Agent::Claude,
            br#"{"claudeAiOauth":{"accessToken":"secret-token"}}"#,
            None,
            "test",
        )
        .unwrap();

        let dir = slot_dir(Agent::Claude, &label);
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir) & 0o077, 0, "slot dir mode {:o}", mode(&dir));
        let cred = dir.join(".credentials.json");
        assert_eq!(mode(&cred) & 0o077, 0, "cred mode {:o}", mode(&cred));
        let meta = dir.join("account.json");
        assert_eq!(mode(&meta) & 0o077, 0, "meta mode {:o}", mode(&meta));

        std::env::remove_var("AI_HANDOFF_HOME");
    }

    #[test]
    #[cfg(unix)]
    fn switch_slot_keeps_live_credential_private() {
        use std::os::unix::fs::PermissionsExt;
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let codex = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CODEX_HOME", codex.path());

        let live = codex.path().join("auth.json");
        std::fs::write(
            &live,
            br#"{"tokens":{"id_token":"x","account_id":"alice"}}"#,
        )
        .unwrap();
        let label = snapshot_current(Agent::Codex).unwrap();
        switch_slot(Agent::Codex, &label).unwrap();

        let mode = std::fs::metadata(&live).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode & 0o077,
            0,
            "live credential must stay private after a switch, got {mode:o}"
        );

        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CODEX_HOME");
    }

    #[test]
    fn parse_process_names_reads_listing() {
        // Windows tasklist CSV (already lowercased before parsing).
        let csv = "\"codex.exe\",\"1234\",\"console\",\"1\",\"50,000 k\"\n\"explorer.exe\",\"42\",\"console\",\"1\",\"9 k\"\n";
        // Unix `ps -o comm=` style.
        let ps = "codex\nclaude\nbash\n";
        let names = if cfg!(windows) {
            parse_process_names(csv)
        } else {
            parse_process_names(ps)
        };
        assert!(names.iter().any(|n| n.contains("codex")));
    }

    #[test]
    fn sanitize_keeps_emails_and_drops_separators() {
        assert_eq!(sanitize("test@test.com"), "test@test.com");
        assert_eq!(sanitize("a b/c\\d"), "a_b_c_d");
        assert_eq!(sanitize("///"), "account");
    }

    #[test]
    fn pool_snapshot_list_switch_delete_roundtrip() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        let codex = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CODEX_HOME", codex.path());

        // Two distinct live auth files captured as two slots.
        let live = codex.path().join("auth.json");
        std::fs::write(
            &live,
            br#"{"tokens":{"id_token":"x","account_id":"alice"}}"#,
        )
        .unwrap();
        let a = snapshot_current(Agent::Codex).unwrap();
        assert_eq!(a, "alice");

        std::fs::write(&live, br#"{"tokens":{"id_token":"y","account_id":"bob"}}"#).unwrap();
        let b = snapshot_current(Agent::Codex).unwrap();
        assert_eq!(b, "bob");

        // Live currently equals "bob"; the list marks it active and carries meta.
        let slots = list_slots(Agent::Codex);
        assert_eq!(slots.len(), 2);
        let bob = slots.iter().find(|s| s.meta.label == "bob").unwrap();
        assert!(bob.active, "bob snapshot matches live bytes");
        assert_eq!(bob.meta.account_id.as_deref(), Some("bob"));
        assert_eq!(bob.meta.source.as_deref(), Some("capture-current"));
        assert!(
            !slots
                .iter()
                .find(|s| s.meta.label == "alice")
                .unwrap()
                .active
        );

        // Switch back to alice: the live file now matches the alice snapshot.
        switch_slot(Agent::Codex, "alice").unwrap();
        let live_bytes = std::fs::read(&live).unwrap();
        assert!(live_bytes.windows(5).any(|w| w == b"alice"));
        assert!(
            list_slots(Agent::Codex)
                .iter()
                .find(|s| s.meta.label == "alice")
                .unwrap()
                .active
        );

        // Delete bob; only alice remains (idempotent on a second delete).
        delete_slot(Agent::Codex, "bob").unwrap();
        delete_slot(Agent::Codex, "bob").unwrap();
        let slots = list_slots(Agent::Codex);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].meta.label, "alice");

        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CODEX_HOME");
    }
}
