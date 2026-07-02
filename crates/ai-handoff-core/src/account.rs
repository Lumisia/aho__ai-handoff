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
//!   `~/.codex/auth.json` (`tokens.id_token`), decoded locally. The raw token
//!   is never returned except by [`codex_request_auth`], used only by the
//!   network module; it is never logged.
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

/// The `(access_token, account_id)` needed for an authenticated backend call.
///
/// **Secret material.** The only caller is the network module that fetches the
/// reset-credit count; the token must never be logged, displayed, or passed to
/// any agent. Returns `None` when not signed in.
pub fn codex_request_auth() -> Option<(String, Option<String>)> {
    request_auth_from_path(&codex_home()?.join("auth.json"))
}

/// The `(access_token, account_id)` stored in a saved slot — used to fetch that
/// account's own usage. Codex only (Claude's `.credentials.json` has no bearer
/// token of this shape, so this returns `None` for Claude).
pub fn slot_request_auth(agent: Agent, label: &str) -> Option<(String, Option<String>)> {
    request_auth_from_path(&slot_dir(agent, label).join(cred_filename(agent)))
}

pub fn claude_slot_oauth(label: &str) -> Option<(String, Option<String>)> {
    claude_oauth_from_path(&slot_dir(Agent::Claude, label).join(cred_filename(Agent::Claude)))
}

/// The live (active) Claude OAuth `(access_token, plan)` from
/// `~/.claude/.credentials.json`.
///
/// **Secret material.** Only for the usage fetcher; the token must never be
/// logged, displayed, or passed to any agent. Returns `None` when not signed in.
pub fn claude_live_oauth() -> Option<(String, Option<String>)> {
    claude_oauth_from_path(&live_auth_path(Agent::Claude)?)
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
    let value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
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

/// List saved account slots, marking which one matches the live credential.
pub fn list_slots(agent: Agent) -> Vec<AccountSlot> {
    let root = accounts_root(agent);
    let live = live_auth_path(agent).and_then(|p| std::fs::read(p).ok());
    let mut slots = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return slots;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let cred = match std::fs::read(dir.join(cred_filename(agent))) {
            Ok(b) => b,
            Err(_) => continue, // not a credential slot
        };
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
        });
        let active = live.as_ref().map(|l| *l == cred).unwrap_or(false);
        slots.push(AccountSlot { meta, dir, active });
    }
    slots.sort_by(|a, b| a.meta.label.cmp(&b.meta.label));
    slots
}

/// Capture the agent's current live credential into a new slot (with metadata).
/// Returns the slot label.
pub fn snapshot_current(agent: Agent) -> std::io::Result<String> {
    let live = live_auth_path(agent).ok_or_else(|| std::io::Error::other("no home dir"))?;
    let bytes = std::fs::read(&live)?;
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
    let base_label = sanitize(&label_from_identity(agent, identity));
    let label = resolve_slot_label(agent, &base_label, cred_bytes, identity);
    let dir = slot_dir(agent, &label);
    std::fs::create_dir_all(&dir)?;
    atomic_write(&dir.join(cred_filename(agent)), cred_bytes)?;
    let now = now_rfc3339();
    let meta = AccountMeta {
        schema_version: 1,
        agent: agent.dir().to_string(),
        label: label.clone(),
        email: identity.and_then(|i| i.email.clone()),
        plan_hint: identity.and_then(|i| i.plan_type.clone()),
        account_id: identity.and_then(|i| i.account_id.clone()),
        workspace_id: None,
        created_at: Some(now.clone()),
        last_verified_at: Some(now),
        source: Some(source.to_string()),
    };
    let json = serde_json::to_vec_pretty(&meta).map_err(std::io::Error::other)?;
    atomic_write(&dir.join("account.json"), &json)?;
    Ok(label)
}

fn resolve_slot_label(
    agent: Agent,
    base_label: &str,
    cred_bytes: &[u8],
    identity: Option<&Identity>,
) -> String {
    if slot_can_be_reused(agent, base_label, cred_bytes, identity) {
        return base_label.to_string();
    }

    if let Some(email) = identity.and_then(|i| i.email.as_deref()) {
        let email_label = sanitize(email);
        if email_label != base_label
            && slot_can_be_reused(agent, &email_label, cred_bytes, identity)
        {
            return email_label;
        }
    }

    for n in 2.. {
        let label = format!("{base_label}-{n}");
        if slot_can_be_reused(agent, &label, cred_bytes, identity) {
            return label;
        }
    }
    unreachable!("unbounded suffix search must find a slot label")
}

fn slot_can_be_reused(
    agent: Agent,
    label: &str,
    cred_bytes: &[u8],
    identity: Option<&Identity>,
) -> bool {
    let dir = slot_dir(agent, label);
    if !dir.exists() {
        return true;
    }
    if std::fs::read(dir.join(cred_filename(agent)))
        .map(|existing| existing == cred_bytes)
        .unwrap_or(false)
    {
        return true;
    }
    read_meta(&dir)
        .as_ref()
        .is_some_and(|meta| meta_matches_identity(meta, identity))
}

fn meta_matches_identity(meta: &AccountMeta, identity: Option<&Identity>) -> bool {
    let Some(identity) = identity else {
        return false;
    };
    let mut matched_field = false;
    if let Some(email) = identity.email.as_deref() {
        if let Some(existing) = meta.email.as_deref() {
            matched_field = true;
            if !existing.eq_ignore_ascii_case(email) {
                return false;
            }
        }
    }
    if let Some(account_id) = identity.account_id.as_deref() {
        if let Some(existing) = meta.account_id.as_deref() {
            matched_field = true;
            if existing != account_id {
                return false;
            }
        }
    }
    matched_field
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

/// Capture a completed vendor-CLI login, then make that saved slot the active
/// local credential. This is the "add account" path used by the UIs.
pub fn capture_login_as_active(
    agent: Agent,
    profile_home: &Path,
    source: &str,
) -> std::io::Result<String> {
    let label = capture_login(agent, profile_home, source)?;
    switch_slot(agent, &label)?;
    Ok(label)
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
    // would not change the live login. Guard until a Keychain adapter exists.
    #[cfg(target_os = "macos")]
    if agent == Agent::Claude && live_auth_path(agent).map(|p| !p.exists()).unwrap_or(false) {
        return Err(std::io::Error::other(
            "macOS Claude stores credentials in the Keychain; file switch isn't supported yet — use launch (l)",
        ));
    }
    let dir = slot_dir(agent, label);
    let bytes = std::fs::read(dir.join(cred_filename(agent)))?;
    let live = live_auth_path(agent).ok_or_else(|| std::io::Error::other("no home dir"))?;
    if let Some(parent) = live.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic_write(&live, &bytes)?;
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
    #[cfg(windows)]
    let output = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output();
    #[cfg(not(windows))]
    let output = std::process::Command::new("ps")
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

/// Write `bytes` to `target` atomically (tmp in the same dir, then rename).
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
    fn capture_login_as_active_switches_claude_to_saved_slot() {
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

        let label =
            capture_login_as_active(Agent::Claude, profile.path(), "official-cli-login").unwrap();

        assert_eq!(label, "dev@example.com");
        assert_eq!(
            std::fs::read(live.path().join(".credentials.json")).unwrap(),
            credential
        );
        let slots = list_slots(Agent::Claude);
        assert_eq!(slots.len(), 1);
        assert!(slots[0].active);
        assert_eq!(slots[0].meta.email.as_deref(), Some("dev@example.com"));

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
    fn save_slot_preserves_existing_slot_when_label_collides_with_different_email() {
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
        assert_eq!(second, "h2171@gmail.com");
        let slots = list_slots(Agent::Codex);
        assert_eq!(slots.len(), 2);
        assert_eq!(
            std::fs::read(slot_dir(Agent::Codex, &first).join("auth.json")).unwrap(),
            b"naver-credential"
        );
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
