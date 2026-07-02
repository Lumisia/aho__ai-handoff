#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use ai_handoff_core::{
    account::{self, Agent, RateWindow},
    capsule::{Capsule, ConsumptionState},
    capsule_codec::{self, CapsuleCodecError},
    config,
    dashboard::{self, CapsuleList, DashboardSnapshot, LogFile, ReadTextResult},
    paths, secure_fs,
};
use ai_handoff_ipc::{
    client::{send, ClientConfig},
    protocol::{ClientInfo, Request, Status, VERSION},
};
use ai_handoff_usage::{
    aggregate::{self, Group},
    model::{Tokens, UsageEvent},
    Dimension,
};
use serde::Serialize;

const TEXT_LIMIT: u64 = 512 * 1024;
const DAY_BREAKDOWN_WINDOW_DAYS: i64 = 30;
static USAGE_SCAN_CACHE: OnceLock<Mutex<ai_handoff_usage::ScanCache>> = OnceLock::new();

async fn blocking_command<T, F>(label: &'static str, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|error| format!("{label} worker failed: {error}"))?
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
struct ConfigRow {
    key: String,
    value: String,
    default_value: String,
    kind: String,
    category: String,
    description: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct UsageReport {
    total: UsageGroup,
    by_source: Vec<UsageGroup>,
    by_day: Vec<UsageGroup>,
    by_model: Vec<UsageGroup>,
    by_project: Vec<UsageGroup>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct UsageGroup {
    key: String,
    tokens: UsageTokens,
    cost_usd: f64,
    unpriced_tokens: u64,
    events: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
struct UsageTokens {
    input: u64,
    cache_read: u64,
    cache_write: u64,
    output: u64,
    total: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct AccountReport {
    codex: AccountAgentReport,
    claude: AccountAgentReport,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct AccountAgentReport {
    agent: String,
    root: String,
    active: Option<String>,
    plan: Option<String>,
    five_hour: Option<AccountWindow>,
    weekly: Option<AccountWindow>,
    slots: Vec<AccountSlotRow>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct AccountSlotRow {
    label: String,
    email: Option<String>,
    plan: Option<String>,
    account_id: Option<String>,
    source: Option<String>,
    created_at: Option<String>,
    active: bool,
    path: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct AccountWindow {
    used_percent: f64,
    remaining_percent: f64,
    window_minutes: u64,
    resets_at: Option<i64>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
struct AccountLoginSession {
    agent: String,
    home: String,
    message: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct AccountLoginPoll {
    done: bool,
    message: String,
    label: Option<String>,
    report: Option<AccountReport>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct AccountOpResult {
    message: String,
    report: AccountReport,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct SlotUsageReport {
    plan: Option<String>,
    five_hour: Option<AccountWindow>,
    weekly: Option<AccountWindow>,
    reset_credits: Option<i64>,
    reset_credit_details: Vec<ResetCreditRow>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
struct ResetCreditRow {
    granted_at: String,
    expires_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AccountCommandSpec {
    env_var: &'static str,
    home: PathBuf,
    program: &'static str,
    args: Vec<&'static str>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
struct ThemeReport {
    language: String,
    preset: String,
    codex_color: String,
    claude_color: String,
    focus_border_color: String,
    selection_bg_color: String,
    selection_fg_color: String,
    app_bg_color: String,
    sidebar_bg_color: String,
    panel_bg_color: String,
    text_color: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct IntegrationReport {
    snapshot: DashboardSnapshot,
    doctor: DoctorSummary,
    repairs: Vec<RepairAction>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
struct DoctorSummary {
    daemon: String,
    ok: usize,
    warn: usize,
    fail: usize,
    codex_accounts: usize,
    claude_accounts: usize,
    elapsed_ms: u128,
    lines: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
struct RepairAction {
    id: String,
    label: String,
    detail: String,
    command: Option<Vec<String>>,
    requires_confirm: bool,
    manual: bool,
    recommended_by: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
struct RepairRunResult {
    action: RepairAction,
    exit_code: Option<i32>,
    output: String,
    report: IntegrationReport,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
struct MenuCommandResult {
    message: String,
}

#[tauri::command]
async fn get_dashboard_snapshot() -> Result<DashboardSnapshot, String> {
    blocking_command("get_dashboard_snapshot", || {
        Ok(dashboard::dashboard_snapshot())
    })
    .await
}

#[tauri::command]
async fn list_capsules() -> Result<CapsuleList, String> {
    blocking_command("list_capsules", || Ok(dashboard::list_capsules())).await
}

#[tauri::command]
async fn read_capsule(path: String) -> Result<ReadTextResult, String> {
    blocking_command("read_capsule", move || {
        Ok(dashboard::read_capsule(&PathBuf::from(path), TEXT_LIMIT))
    })
    .await
}

#[tauri::command]
async fn read_logs() -> Result<Vec<LogFile>, String> {
    blocking_command("read_logs", || Ok(dashboard::read_logs(TEXT_LIMIT))).await
}

#[tauri::command]
async fn get_usage_report() -> Result<UsageReport, String> {
    blocking_command("get_usage_report", || {
        let mut cache = USAGE_SCAN_CACHE
            .get_or_init(|| Mutex::new(ai_handoff_usage::ScanCache::default()))
            .lock()
            .map_err(|_| "usage cache lock poisoned".to_string())?;
        let roots = ai_handoff_usage::default_roots();
        Ok(usage_report_from_events(&ai_handoff_usage::scan_cached(
            &roots, &mut cache,
        )))
    })
    .await
}

#[tauri::command]
async fn get_account_report(force: Option<bool>) -> Result<AccountReport, String> {
    let force = force.unwrap_or(false);
    blocking_command("get_account_report", move || Ok(account_report(force))).await
}

#[tauri::command]
async fn get_integration_report() -> Result<IntegrationReport, String> {
    blocking_command("get_integration_report", || Ok(integration_report())).await
}

#[tauri::command]
fn start_account_login(agent: String) -> Result<AccountLoginSession, String> {
    start_account_login_for(parse_agent(&agent)?)
}

#[tauri::command]
fn poll_account_login(agent: String, home: String) -> Result<AccountLoginPoll, String> {
    poll_account_login_at(parse_agent(&agent)?, &PathBuf::from(home))
}

#[tauri::command]
fn launch_account_slot(agent: String, label: String) -> Result<AccountOpResult, String> {
    launch_account_slot_for(parse_agent(&agent)?, &label)
}

#[tauri::command]
async fn refresh_account_slot_usage(
    agent: String,
    label: String,
) -> Result<SlotUsageReport, String> {
    blocking_command("refresh_account_slot_usage", move || {
        let usage = ai_handoff_tui::account_api::fetch_slot_usage(parse_agent(&agent)?, &label)?;
        Ok(slot_usage_report_from_data(usage))
    })
    .await
}

#[tauri::command]
async fn run_repair_action(action_id: String) -> Result<RepairRunResult, String> {
    blocking_command("run_repair_action", move || {
        run_repair_action_by_id(&action_id)
    })
    .await
}

#[tauri::command]
fn capture_current_account(agent: String) -> Result<AccountReport, String> {
    account::snapshot_current(parse_agent(&agent)?).map_err(|error| error.to_string())?;
    Ok(account_report(false))
}

#[tauri::command]
fn switch_account_slot(agent: String, label: String) -> Result<AccountReport, String> {
    account::switch_slot(parse_agent(&agent)?, &label).map_err(|error| error.to_string())?;
    Ok(account_report(false))
}

#[tauri::command]
fn delete_account_slot(agent: String, label: String) -> Result<AccountReport, String> {
    account::delete_slot(parse_agent(&agent)?, &label).map_err(|error| error.to_string())?;
    Ok(account_report(false))
}

#[tauri::command]
async fn get_config_settings() -> Result<Vec<ConfigRow>, String> {
    blocking_command("get_config_settings", || {
        config_rows_for(&paths::config_path())
    })
    .await
}

#[tauri::command]
async fn get_theme() -> Result<ThemeReport, String> {
    blocking_command("get_theme", || {
        Ok(theme_report_from_config(&config::load()))
    })
    .await
}

#[tauri::command]
async fn set_config_value(key: String, value: String) -> Result<Vec<ConfigRow>, String> {
    blocking_command("set_config_value", move || {
        let path = paths::config_path();
        set_config_at(&path, &key, &value)?;
        config_rows_for(&path)
    })
    .await
}

#[tauri::command]
async fn reset_config_value(key: String) -> Result<Vec<ConfigRow>, String> {
    blocking_command("reset_config_value", move || {
        let path = paths::config_path();
        reset_config_at(&path, &key)?;
        config_rows_for(&path)
    })
    .await
}

#[tauri::command]
async fn toggle_capsule_state(path: String) -> Result<String, String> {
    blocking_command("toggle_capsule_state", move || {
        toggle_capsule_state_at(&PathBuf::from(path))
    })
    .await
}

#[tauri::command]
async fn set_capsule_state(path: String, state: String) -> Result<String, String> {
    blocking_command("set_capsule_state", move || {
        set_capsule_state_at(&PathBuf::from(path), &state)
    })
    .await
}

#[tauri::command]
async fn set_capsule_field(path: String, field: String, value: String) -> Result<(), String> {
    blocking_command("set_capsule_field", move || {
        set_capsule_field_at(&PathBuf::from(path), &field, &value)
    })
    .await
}

#[tauri::command]
async fn delete_capsule(path: String) -> Result<(), String> {
    blocking_command("delete_capsule", move || {
        std::fs::remove_file(PathBuf::from(path)).map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
async fn open_capsule_folder(path: String) -> Result<MenuCommandResult, String> {
    blocking_command("open_capsule_folder", move || {
        let capsule_path = PathBuf::from(path);
        let folder = capsule_path
            .parent()
            .ok_or_else(|| "capsule path has no parent folder".to_string())?;
        open_target(&folder.to_string_lossy())?;
        Ok(MenuCommandResult {
            message: format!("opened folder: {}", folder.display()),
        })
    })
    .await
}

#[tauri::command]
async fn open_capsule_external(path: String) -> Result<MenuCommandResult, String> {
    blocking_command("open_capsule_external", move || {
        open_file_with_picker(&PathBuf::from(path))?;
        Ok(MenuCommandResult {
            message: "opened capsule with external app picker".into(),
        })
    })
    .await
}

#[tauri::command]
async fn run_doctor() -> Result<IntegrationReport, String> {
    blocking_command("run_doctor", || Ok(integration_report())).await
}

#[tauri::command]
async fn create_checkpoint() -> Result<MenuCommandResult, String> {
    blocking_command("create_checkpoint", || {
        let output = run_cli_capture(
            &[
                "checkpoint",
                "--agent",
                "codex",
                "--message",
                "GUI checkpoint",
            ],
            false,
        )?;
        Ok(MenuCommandResult {
            message: format!("checkpoint saved: {output}"),
        })
    })
    .await
}

#[tauri::command]
async fn open_logs_folder() -> Result<MenuCommandResult, String> {
    blocking_command("open_logs_folder", || {
        let logs = paths::logs_dir();
        std::fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
        open_target(&logs.to_string_lossy())?;
        Ok(MenuCommandResult {
            message: format!("opened logs folder: {}", logs.display()),
        })
    })
    .await
}

#[tauri::command]
async fn reinstall_hooks() -> Result<MenuCommandResult, String> {
    blocking_command("reinstall_hooks", || {
        let output = run_cli_capture(&["install", "--yes"], false)?;
        Ok(MenuCommandResult {
            message: format!("hooks reinstall completed: {output}"),
        })
    })
    .await
}

#[tauri::command]
async fn ensure_daemon_running() -> Result<MenuCommandResult, String> {
    blocking_command("ensure_daemon_running", || {
        if probe_daemon() == "reachable" {
            return Ok(MenuCommandResult {
                message: "daemon already reachable".into(),
            });
        }
        let exe = cli_executable()?;
        let child = hidden_command(&exe)
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        Ok(MenuCommandResult {
            message: format!("daemon started: pid {}", child.id()),
        })
    })
    .await
}

#[tauri::command]
async fn open_project_github() -> Result<MenuCommandResult, String> {
    blocking_command("open_project_github", || {
        open_target("https://github.com/Lumisia/aho__ai-handoff")?;
        Ok(MenuCommandResult {
            message: "opened GitHub repository".into(),
        })
    })
    .await
}

fn config_rows_for(path: &Path) -> Result<Vec<ConfigRow>, String> {
    let cfg = config::load_from(path);
    desktop_config_keys()
        .into_iter()
        .map(|key| {
            let kind = config::key_kind(key).ok_or_else(|| format!("unknown config key: {key}"))?;
            let value = config::get_value(&cfg, key).map_err(|error| error.to_string())?;
            let default_value = config::default_value(key).map_err(|error| error.to_string())?;
            Ok(ConfigRow {
                key: key.to_string(),
                value,
                default_value,
                kind: key_kind_name(kind).to_string(),
                category: category_for_key(key).to_string(),
                description: description_for_key(key).to_string(),
            })
        })
        .collect()
}

fn desktop_config_keys() -> Vec<&'static str> {
    config::settable_keys()
        .filter(|key| !key.starts_with("theme."))
        .chain(config::gui_settable_keys())
        .collect()
}

fn set_config_at(path: &Path, key: &str, value: &str) -> Result<(), String> {
    let existing = std::fs::read_to_string(path).ok();
    let text =
        config::set_value(existing.as_deref(), key, value).map_err(|error| error.to_string())?;
    write_config_atomic(path, &text)
}

fn reset_config_at(path: &Path, key: &str) -> Result<(), String> {
    let value = config::default_value(key).map_err(|error| error.to_string())?;
    set_config_at(path, key, &value)
}

fn write_config_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("toml.tmp");
    secure_fs::write_private_atomic(path, &tmp, text.as_bytes()).map_err(|error| error.to_string())
}

fn key_kind_name(kind: config::KeyKind) -> &'static str {
    match kind {
        config::KeyKind::Bool => "bool",
        config::KeyKind::Percent => "percent",
        config::KeyKind::PosFloat => "positive_float",
        config::KeyKind::Count => "count",
        config::KeyKind::Mode => "mode",
        config::KeyKind::Lang => "language",
        config::KeyKind::CapsuleFormat => "capsule_format",
        config::KeyKind::ThemePreset => "theme_preset",
        config::KeyKind::GuiThemePreset => "gui_theme_preset",
        config::KeyKind::Color => "color",
    }
}

fn category_for_key(key: &str) -> &'static str {
    if key.starts_with("triggers.") {
        "triggers"
    } else if key == "capsule.language" {
        "language"
    } else if key.starts_with("capsule.") {
        "capsule"
    } else if key == "language" {
        "language"
    } else if key.starts_with("gui_theme.") || key.starts_with("theme.") {
        "theme"
    } else if key.starts_with("autostart.") {
        "automation"
    } else if key.starts_with("statusline.") {
        "agents"
    } else {
        "advanced"
    }
}

fn description_for_key(key: &str) -> &'static str {
    match key {
        "triggers.five_hour.enabled" => "Enable handoff trigger checks for five-hour limits.",
        "triggers.five_hour.threshold_percent" => {
            "Warn or ask when the active session reaches this usage percent."
        }
        "triggers.five_hour.mode" => {
            "Choose whether triggers are off, ask first, or run automatically."
        }
        "triggers.five_hour.burn_rate.enabled" => "Use recent burn rate to estimate runway.",
        "triggers.five_hour.burn_rate.runway_minutes" => {
            "Warn when estimated runway falls under this many minutes."
        }
        "autostart.enabled" => "Start the daemon automatically when the user logs in.",
        "statusline.show" => "Show ai-handoff status in Claude Code statusline.",
        "language" => "Preferred UI language for shared ai-handoff surfaces.",
        "capsule.format" => "Choose JSON or Markdown for newly written capsules.",
        "capsule.language" => "Sets the language used when creating capsules.",
        "capsule.next_prompt_max_items" => {
            "Maximum next-prompt entries generated by checkpoint guidance."
        }
        "capsule.remaining_max_items" => {
            "Maximum remaining-work entries generated by checkpoint guidance."
        }
        "capsule.done_max_items" => {
            "Maximum completed-work entries generated by checkpoint guidance."
        }
        "capsule.risks_max_items" => "Maximum risk entries generated by checkpoint guidance.",
        "theme.preset" => "Base color preset used by the TUI.",
        "theme.codex_color" => "TUI color for Codex labels and usage marks.",
        "theme.claude_color" => "TUI color for Claude labels and usage marks.",
        "theme.focus_border_color" => "TUI color for focused subpanes.",
        "theme.selection_bg_color" => "TUI background color for selected rows and tabs.",
        "theme.selection_fg_color" => "TUI text color for selected rows and tabs.",
        "gui_theme.preset" => "Base visual preset used by the desktop GUI.",
        "gui_theme.codex_color" => "GUI color for Codex labels, marks, and capsules.",
        "gui_theme.claude_color" => "GUI color for Claude labels, marks, and capsules.",
        "gui_theme.focus_border_color" => "GUI color for focused panes and keyboard focus.",
        "gui_theme.selection_bg_color" => "GUI background color for selected rows and tabs.",
        "gui_theme.selection_fg_color" => "GUI text color for selected rows and tabs.",
        "gui_theme.app_bg_color" => "GUI main application background color.",
        "gui_theme.sidebar_bg_color" => "GUI sidebar background color.",
        "gui_theme.panel_bg_color" => "GUI card, panel, table, and menu background color.",
        "gui_theme.text_color" => "GUI primary text color.",
        _ => "Raw editable setting.",
    }
}

fn toggle_capsule_state_at(path: &Path) -> Result<String, String> {
    let mut capsule = read_capsule_typed(path)?;
    let new = capsule.consumption.state.next();
    save_capsule_state(path, &mut capsule, new)
}

fn set_capsule_state_at(path: &Path, state: &str) -> Result<String, String> {
    let mut capsule = read_capsule_typed(path)?;
    let new = parse_consumption_state(state)?;
    save_capsule_state(path, &mut capsule, new)
}

fn save_capsule_state(
    path: &Path,
    capsule: &mut Capsule,
    new: ConsumptionState,
) -> Result<String, String> {
    capsule.consumption.state = new;
    if new == ConsumptionState::Consumed {
        capsule.consumption.consumed_by = Some("ai-handoff-gui".to_string());
        capsule.consumption.consumed_at =
            Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    } else {
        capsule.consumption.consumed_by = None;
        capsule.consumption.consumed_at = None;
    }
    write_capsule_typed(path, capsule)?;
    Ok(new.as_str().to_string())
}

fn parse_consumption_state(state: &str) -> Result<ConsumptionState, String> {
    match state {
        "pending" => Ok(ConsumptionState::Pending),
        "in_progress" => Ok(ConsumptionState::InProgress),
        "blocked" => Ok(ConsumptionState::Blocked),
        "needs_review" => Ok(ConsumptionState::NeedsReview),
        "consumed" => Ok(ConsumptionState::Consumed),
        "archived" => Ok(ConsumptionState::Archived),
        _ => Err(format!("unknown capsule state: {state}")),
    }
}

fn set_capsule_field_at(path: &Path, field: &str, value: &str) -> Result<(), String> {
    let mut capsule = read_capsule_typed(path)?;
    match field {
        "goal" => capsule.summary.goal = value.to_string(),
        "next_prompt" => {
            capsule.next_prompt = if value.trim().is_empty() {
                None
            } else {
                Some(value.to_string())
            };
        }
        "remaining" => capsule.summary.remaining = split_items(value),
        "done" => capsule.summary.done = split_items(value),
        "risks" => capsule.summary.risks = split_items(value),
        _ => return Err(format!("unknown capsule field: {field}")),
    }
    write_capsule_typed(path, &capsule)
}

fn read_capsule_typed(path: &Path) -> Result<Capsule, String> {
    capsule_codec::read_capsule(path).map_err(capsule_error)
}

fn write_capsule_typed(path: &Path, capsule: &Capsule) -> Result<(), String> {
    capsule_codec::write_capsule(path, capsule, capsule_format_for_path(path))
        .map_err(capsule_error)
}

fn capsule_format_for_path(path: &Path) -> config::CapsuleFormat {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("md") => config::CapsuleFormat::Md,
        _ => config::CapsuleFormat::Json,
    }
}

fn capsule_error(error: CapsuleCodecError) -> String {
    match error {
        CapsuleCodecError::Io(error) => error.to_string(),
        other => other.to_string(),
    }
}

fn split_items(value: &str) -> Vec<String> {
    value
        .split('|')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn usage_report_from_events(events: &[UsageEvent]) -> UsageReport {
    usage_report_from_events_for_today(events, chrono::Local::now().date_naive())
}

fn usage_report_from_events_for_today(
    events: &[UsageEvent],
    today: chrono::NaiveDate,
) -> UsageReport {
    let since = today - chrono::Duration::days(DAY_BREAKDOWN_WINDOW_DAYS - 1);
    let since = since.format("%Y-%m-%d").to_string();
    let through = today.format("%Y-%m-%d").to_string();
    let recent_events = events
        .iter()
        .filter(|event| {
            let day = event.day.as_str();
            day >= since.as_str() && day <= through.as_str()
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut grouped_by_day = aggregate::group_by(&recent_events, Dimension::Day)
        .into_iter()
        .map(|group| (group.key.clone(), group))
        .collect::<std::collections::HashMap<_, _>>();
    let by_day = (0..DAY_BREAKDOWN_WINDOW_DAYS)
        .map(|offset| {
            let key = (today - chrono::Duration::days(offset))
                .format("%Y-%m-%d")
                .to_string();
            group_to_usage(grouped_by_day.remove(&key).unwrap_or_else(|| Group {
                key,
                tokens: Tokens::default(),
                cost_usd: 0.0,
                unpriced_tokens: 0,
                events: 0,
            }))
        })
        .collect();

    UsageReport {
        total: group_to_usage(aggregate::totals(events)),
        by_source: aggregate::group_by(events, Dimension::Source)
            .into_iter()
            .map(group_to_usage)
            .collect(),
        by_day,
        by_model: aggregate::group_by(events, Dimension::Model)
            .into_iter()
            .map(group_to_usage)
            .collect(),
        by_project: aggregate::group_by(events, Dimension::Project)
            .into_iter()
            .map(group_to_usage)
            .collect(),
    }
}

fn group_to_usage(group: Group) -> UsageGroup {
    UsageGroup {
        key: group.key,
        tokens: tokens_to_usage(group.tokens),
        cost_usd: group.cost_usd,
        unpriced_tokens: group.unpriced_tokens,
        events: group.events,
    }
}

fn tokens_to_usage(tokens: Tokens) -> UsageTokens {
    UsageTokens {
        input: tokens.input,
        cache_read: tokens.cache_read,
        cache_write: tokens.cache_write,
        output: tokens.output,
        total: tokens.total(),
    }
}

fn account_report(force: bool) -> AccountReport {
    AccountReport {
        codex: account_agent_report(
            Agent::Codex,
            agent_status_with_fallback(Agent::Codex, account::codex_status(), force),
        ),
        claude: account_agent_report(
            Agent::Claude,
            agent_status_with_fallback(Agent::Claude, account::claude_status(), force),
        ),
    }
}

/// Local status (statusline samples / rollout logs) is only available while a
/// session has recently run. When it is missing ??or the user forces a refresh
/// ??fall back to the account's own usage endpoint so the panel still shows
/// live 5h/weekly limits.
fn agent_status_with_fallback(
    agent: Agent,
    local: Option<account::AccountStatus>,
    force: bool,
) -> Option<account::AccountStatus> {
    let missing_windows = local
        .as_ref()
        .map(|status| status.five_hour.is_none() && status.weekly.is_none())
        .unwrap_or(true);
    if !force && !missing_windows {
        return local;
    }
    match ai_handoff_tui::account_api::fetch_live_usage(agent) {
        Ok(usage) => Some(account::AccountStatus {
            plan_type: usage
                .plan
                .clone()
                .or_else(|| local.as_ref().and_then(|s| s.plan_type.clone())),
            five_hour: usage.five_hour,
            weekly: usage.weekly,
            captured_at: Some(chrono::Utc::now().timestamp_millis()),
        }),
        Err(_) => local,
    }
}

fn account_agent_report(
    agent: Agent,
    status: Option<account::AccountStatus>,
) -> AccountAgentReport {
    let slots = account::list_slots(agent)
        .into_iter()
        .map(|slot| AccountSlotRow {
            label: slot.meta.label,
            email: slot.meta.email,
            plan: slot.meta.plan_hint,
            account_id: slot.meta.account_id,
            source: slot.meta.source,
            created_at: slot.meta.created_at,
            active: slot.active,
            path: slot.dir.display().to_string(),
        })
        .collect::<Vec<_>>();
    let active = slots
        .iter()
        .find(|slot| slot.active)
        .map(|slot| slot.label.clone());
    let plan = status.as_ref().and_then(|status| status.plan_type.clone());

    AccountAgentReport {
        agent: agent_name(agent).to_string(),
        root: paths::home()
            .join("accounts")
            .join(agent_name(agent))
            .display()
            .to_string(),
        active,
        plan,
        five_hour: status
            .as_ref()
            .and_then(|status| status.five_hour.as_ref())
            .map(account_window),
        weekly: status
            .as_ref()
            .and_then(|status| status.weekly.as_ref())
            .map(account_window),
        slots,
    }
}

fn account_window(window: &RateWindow) -> AccountWindow {
    AccountWindow {
        used_percent: window.used_percent,
        remaining_percent: window.remaining_percent(),
        window_minutes: window.window_minutes,
        resets_at: window.resets_at,
    }
}

fn slot_usage_report_from_data(usage: ai_handoff_tui::account_api::UsageData) -> SlotUsageReport {
    SlotUsageReport {
        plan: usage.plan,
        five_hour: usage.five_hour.as_ref().map(account_window),
        weekly: usage.weekly.as_ref().map(account_window),
        reset_credits: usage.reset_credits,
        reset_credit_details: usage
            .reset_credit_details
            .into_iter()
            .map(|credit| ResetCreditRow {
                granted_at: credit.granted_at,
                expires_at: credit.expires_at,
            })
            .collect(),
    }
}

fn agent_name(agent: Agent) -> &'static str {
    match agent {
        Agent::Codex => "codex",
        Agent::Claude => "claude",
    }
}

fn parse_agent(agent: &str) -> Result<Agent, String> {
    match agent {
        "codex" => Ok(Agent::Codex),
        "claude" => Ok(Agent::Claude),
        other => Err(format!("unknown agent: {other}")),
    }
}

fn start_account_login_for(agent: Agent) -> Result<AccountLoginSession, String> {
    let home = temp_login_home(agent)?;
    if agent == Agent::Codex {
        std::fs::write(
            home.join("config.toml"),
            "cli_auth_credentials_store = \"file\"\n",
        )
        .map_err(|error| error.to_string())?;
    }
    let spec = account_login_spec(agent, home.clone());
    spawn_account_login_process(&spec)?;
    Ok(AccountLoginSession {
        agent: agent_name(agent).into(),
        home: home.display().to_string(),
        message: format!("started {} login flow", agent_title(agent)),
    })
}

fn poll_account_login_at(agent: Agent, home: &Path) -> Result<AccountLoginPoll, String> {
    if !account::login_complete(agent, home) {
        return Ok(AccountLoginPoll {
            done: false,
            message: "waiting for official CLI login to write credentials".into(),
            label: None,
            report: None,
        });
    }
    let label = account::capture_login_as_active(agent, home, "official-cli-login")
        .map_err(|error| error.to_string())?;
    Ok(AccountLoginPoll {
        done: true,
        message: format!("captured {} account: {label}", agent_title(agent)),
        label: Some(label),
        report: Some(account_report(false)),
    })
}

fn launch_account_slot_for(agent: Agent, label: &str) -> Result<AccountOpResult, String> {
    let spec = account_launch_spec(agent, label);
    spawn_account_window(&spec)?;
    Ok(AccountOpResult {
        message: format!("launched {} with slot {label}", agent_title(agent)),
        report: account_report(false),
    })
}

fn account_login_spec(agent: Agent, home: PathBuf) -> AccountCommandSpec {
    match agent {
        Agent::Codex => AccountCommandSpec {
            env_var: "CODEX_HOME",
            home,
            program: "codex",
            args: vec!["login"],
        },
        Agent::Claude => AccountCommandSpec {
            env_var: "CLAUDE_CONFIG_DIR",
            home,
            program: "claude",
            args: vec!["auth", "login"],
        },
    }
}

fn account_launch_spec(agent: Agent, label: &str) -> AccountCommandSpec {
    let (env_var, home) = account::profile_env(agent, label);
    AccountCommandSpec {
        env_var,
        home,
        program: match agent {
            Agent::Codex => "codex",
            Agent::Claude => "claude",
        },
        args: vec![],
    }
}

fn spawn_account_window(spec: &AccountCommandSpec) -> Result<(), String> {
    if account::which(spec.program).is_none() {
        return Err(format!("`{}` not found on PATH", spec.program));
    }

    #[cfg(windows)]
    {
        let command_line = account_command_line(spec);
        Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K", &command_line])
            .env(spec.env_var, &spec.home)
            .spawn()
            .map_err(|error| format!("could not open a new window: {error}"))?;
    }
    #[cfg(not(windows))]
    {
        Command::new(spec.program)
            .args(&spec.args)
            .env(spec.env_var, &spec.home)
            .spawn()
            .map_err(|error| format!("could not launch `{}`: {error}", spec.program))?;
    }
    Ok(())
}

fn spawn_account_login_process(spec: &AccountCommandSpec) -> Result<(), String> {
    if account::which(spec.program).is_none() {
        return Err(format!("`{}` not found on PATH", spec.program));
    }

    #[cfg(windows)]
    {
        let command_line = account_command_line(spec);
        hidden_command("cmd")
            .args(["/C", &command_line])
            .env(spec.env_var, &spec.home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("could not start login flow: {error}"))?;
    }
    #[cfg(not(windows))]
    {
        hidden_command(spec.program)
            .args(&spec.args)
            .env(spec.env_var, &spec.home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("could not launch `{}`: {error}", spec.program))?;
    }
    Ok(())
}

fn account_command_line(spec: &AccountCommandSpec) -> String {
    let mut command_line = spec.program.to_string();
    for arg in &spec.args {
        command_line.push(' ');
        command_line.push_str(arg);
    }
    command_line
}

fn temp_login_home(agent: Agent) -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = paths::home()
        .join("tmp")
        .join("login")
        .join(agent_name(agent))
        .join(stamp.to_string());
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn agent_title(agent: Agent) -> &'static str {
    match agent {
        Agent::Codex => "Codex",
        Agent::Claude => "Claude",
    }
}

fn theme_report_from_config(cfg: &config::Config) -> ThemeReport {
    let theme = config::effective_gui_theme_config(&cfg.gui_theme);

    ThemeReport {
        language: config::lang_str(cfg.language).into(),
        preset: gui_theme_preset_name(theme.preset).into(),
        codex_color: css_color(theme.codex_color.as_str()),
        claude_color: css_color(theme.claude_color.as_str()),
        focus_border_color: css_color(theme.focus_border_color.as_str()),
        selection_bg_color: css_color(theme.selection_bg_color.as_str()),
        selection_fg_color: css_color(theme.selection_fg_color.as_str()),
        app_bg_color: css_color(theme.app_bg_color.as_str()),
        sidebar_bg_color: css_color(theme.sidebar_bg_color.as_str()),
        panel_bg_color: css_color(theme.panel_bg_color.as_str()),
        text_color: css_color(theme.text_color.as_str()),
    }
}

fn gui_theme_preset_name(preset: config::GuiThemePreset) -> &'static str {
    match preset {
        config::GuiThemePreset::White => "white",
        config::GuiThemePreset::Dark => "dark",
        config::GuiThemePreset::Custom => "custom",
    }
}

fn css_color(raw: &str) -> String {
    let value = raw.trim();
    if let Some((r, g, b)) = config::ColorSpec::parse(value)
        .ok()
        .and_then(|spec| spec.rgb())
        .or_else(|| indexed_css_rgb(value))
    {
        return format!("#{r:02X}{g:02X}{b:02X}");
    }
    "#FFFFFF".into()
}

fn indexed_css_rgb(raw: &str) -> Option<(u8, u8, u8)> {
    let n: u8 = raw.trim().parse().ok()?;
    match n {
        0 => Some((0, 0, 0)),
        1 => Some((128, 0, 0)),
        2 => Some((0, 128, 0)),
        3 => Some((128, 128, 0)),
        4 => Some((0, 0, 128)),
        5 => Some((128, 0, 128)),
        6 => Some((0, 128, 128)),
        7 => Some((192, 192, 192)),
        8 => Some((128, 128, 128)),
        9 => Some((255, 0, 0)),
        10 => Some((0, 255, 0)),
        11 => Some((255, 255, 0)),
        12 => Some((0, 0, 255)),
        13 => Some((255, 0, 255)),
        14 => Some((0, 255, 255)),
        15 => Some((255, 255, 255)),
        16..=231 => {
            let value = n - 16;
            let r = value / 36;
            let g = (value % 36) / 6;
            let b = value % 6;
            Some((xterm_component(r), xterm_component(g), xterm_component(b)))
        }
        232..=255 => {
            let gray = 8 + (n - 232) * 10;
            Some((gray, gray, gray))
        }
    }
}

fn xterm_component(value: u8) -> u8 {
    if value == 0 {
        0
    } else {
        55 + value * 40
    }
}

fn integration_report() -> IntegrationReport {
    let started = Instant::now();
    let daemon = probe_daemon();
    let snapshot = with_daemon_probe(dashboard::dashboard_snapshot(), &daemon);
    let accounts = account_report(false);
    integration_report_from_parts(snapshot, accounts, daemon, started.elapsed().as_millis())
}

fn integration_report_from_parts(
    snapshot: DashboardSnapshot,
    accounts: AccountReport,
    daemon: String,
    elapsed_ms: u128,
) -> IntegrationReport {
    let doctor = doctor_summary(&snapshot, &accounts, daemon, elapsed_ms);
    let repairs = recommended_repair_actions(&snapshot);
    IntegrationReport {
        snapshot,
        doctor,
        repairs,
    }
}

fn doctor_summary(
    snapshot: &DashboardSnapshot,
    accounts: &AccountReport,
    daemon: String,
    elapsed_ms: u128,
) -> DoctorSummary {
    let mut ok = 0;
    let mut warn = 0;
    let mut fail = 0;
    for check in &snapshot.checks {
        match check.status {
            dashboard::CheckStatus::Ok => ok += 1,
            dashboard::CheckStatus::Warning | dashboard::CheckStatus::Unknown => warn += 1,
            dashboard::CheckStatus::Error | dashboard::CheckStatus::Missing => fail += 1,
        }
    }
    let lines = vec![
        format!("doctor completed in {elapsed_ms}ms"),
        format!("daemon: {daemon}"),
        format!("checks: ok={ok}, warn={warn}, fail={fail}"),
        format!(
            "accounts: codex={}, claude={}",
            accounts.codex.slots.len(),
            accounts.claude.slots.len()
        ),
    ];
    DoctorSummary {
        daemon,
        ok,
        warn,
        fail,
        codex_accounts: accounts.codex.slots.len(),
        claude_accounts: accounts.claude.slots.len(),
        elapsed_ms,
        lines,
    }
}

fn recommended_repair_actions(snapshot: &DashboardSnapshot) -> Vec<RepairAction> {
    let mut actions = Vec::new();
    for check in &snapshot.checks {
        if matches!(
            check.status,
            dashboard::CheckStatus::Error
                | dashboard::CheckStatus::Missing
                | dashboard::CheckStatus::Warning
        ) {
            match check.id.as_str() {
                "codex-hooks" | "claude-settings" | "codex-config" | "ipc" | "store" => {
                    add_repair_action(&mut actions, repair_action("install_plugin"), &check.id)
                }
                "daemon" => {
                    add_repair_action(&mut actions, repair_action("start_daemon"), &check.id)
                }
                "autostart" if snapshot.install_state.autostart != "missing" => {
                    add_repair_action(&mut actions, repair_action("autostart_on"), &check.id)
                }
                _ => {}
            }
        }
    }
    if !snapshot.duplicates.is_empty() {
        add_repair_action(
            &mut actions,
            repair_action("manual_legacy_cleanup"),
            "duplicates",
        );
    }
    if snapshot
        .codex_config
        .message
        .to_ascii_lowercase()
        .contains("trust")
    {
        add_repair_action(
            &mut actions,
            repair_action("manual_codex_trust"),
            "codex-config",
        );
    }
    add_repair_action(&mut actions, repair_action("run_doctor"), "doctor");
    actions
}

fn add_repair_action(actions: &mut Vec<RepairAction>, mut action: RepairAction, source: &str) {
    if let Some(existing) = actions.iter_mut().find(|item| item.id == action.id) {
        if !existing.recommended_by.iter().any(|item| item == source) {
            existing.recommended_by.push(source.to_string());
        }
        return;
    }
    action.recommended_by.push(source.to_string());
    actions.push(action);
}

fn repair_action(id: &str) -> RepairAction {
    match id {
        "install_plugin" => RepairAction {
            id: id.into(),
            label: "Reinstall plugins/hooks".into(),
            detail: "Runs ai-handoff install --yes to reapply Codex/Claude plugins, hooks, writable_roots, and shared environment.".into(),
            command: Some(vec!["install".into(), "--yes".into()]),
            requires_confirm: true,
            manual: false,
            recommended_by: vec![],
        },
        "start_daemon" => RepairAction {
            id: id.into(),
            label: "Start daemon".into(),
            detail: "Starts ai-handoff daemon in the background.".into(),
            command: Some(vec!["daemon".into(), "run".into()]),
            requires_confirm: true,
            manual: false,
            recommended_by: vec![],
        },
        "autostart_on" => RepairAction {
            id: id.into(),
            label: "Enable autostart".into(),
            detail: "Registers ai-handoff daemon to start at logon.".into(),
            command: Some(vec!["autostart".into(), "on".into()]),
            requires_confirm: true,
            manual: false,
            recommended_by: vec![],
        },
        "manual_legacy_cleanup" => RepairAction {
            id: id.into(),
            label: "Manual legacy cleanup".into(),
            detail: "Remove old direct hook or stale plugin-cache entries after checking the listed duplicate paths.".into(),
            command: None,
            requires_confirm: false,
            manual: true,
            recommended_by: vec![],
        },
        "manual_codex_trust" => RepairAction {
            id: id.into(),
            label: "Trust Codex hook".into(),
            detail: "Open Codex /hooks and trust the ai-handoff hook entries shown there.".into(),
            command: None,
            requires_confirm: false,
            manual: true,
            recommended_by: vec![],
        },
        _ => RepairAction {
            id: "run_doctor".into(),
            label: "Run doctor".into(),
            detail: "Refreshes read-only diagnostics and repair recommendations.".into(),
            command: None,
            requires_confirm: false,
            manual: false,
            recommended_by: vec![],
        },
    }
}

fn run_repair_action_by_id(action_id: &str) -> Result<RepairRunResult, String> {
    if action_id == "run_doctor" {
        let report = integration_report();
        let action = report
            .repairs
            .iter()
            .find(|item| item.id == "run_doctor")
            .cloned()
            .unwrap_or_else(|| repair_action("run_doctor"));
        return Ok(RepairRunResult {
            action,
            exit_code: Some(0),
            output: "doctor refreshed".into(),
            report,
        });
    }

    let before = integration_report();
    let action = before
        .repairs
        .iter()
        .find(|item| item.id == action_id)
        .cloned()
        .ok_or_else(|| format!("unknown or not recommended repair action: {action_id}"))?;
    if action.manual {
        return Err(format!(
            "manual action cannot be executed automatically: {action_id}"
        ));
    }
    let args = action
        .command
        .as_ref()
        .ok_or_else(|| format!("repair action has no command: {action_id}"))?;
    let exe = repair_executable(&before.snapshot)?;
    let (exit_code, output) = if action.id == "start_daemon" {
        let child = hidden_command(&exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        (Some(0), format!("spawned pid {}", child.id()))
    } else {
        let output = hidden_command(&exe)
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;
        let code = output.status.code();
        let mut text = String::new();
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        (code, text)
    };
    Ok(RepairRunResult {
        action,
        exit_code,
        output,
        report: integration_report(),
    })
}

fn repair_executable(snapshot: &DashboardSnapshot) -> Result<PathBuf, String> {
    if let Some(launcher) = snapshot.install_state.launcher.as_deref() {
        let path = PathBuf::from(launcher);
        if path.is_file() {
            return Ok(path);
        }
    }
    let bundled = paths::home().join("bin").join(if cfg!(windows) {
        "ai-handoff.exe"
    } else {
        "ai-handoff"
    });
    if bundled.is_file() {
        return Ok(bundled);
    }
    Err("ai-handoff launcher not found; reinstall from CLI first".into())
}

fn with_daemon_probe(mut snapshot: DashboardSnapshot, daemon: &str) -> DashboardSnapshot {
    let status = if daemon == "reachable" {
        dashboard::CheckStatus::Ok
    } else {
        dashboard::CheckStatus::Error
    };
    let row = dashboard::CheckRow {
        id: "daemon".into(),
        label: "Daemon".into(),
        status,
        message: daemon.to_string(),
        path: None,
    };
    snapshot.daemon = row.clone();
    if let Some(existing) = snapshot
        .checks
        .iter_mut()
        .find(|check| check.id == "daemon")
    {
        *existing = row;
    } else {
        snapshot.checks.insert(0, row);
    }
    snapshot
}

fn probe_daemon() -> String {
    let req = Request {
        version: VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: "ping".to_string(),
        agent: "desktop".to_string(),
        event: "ping".to_string(),
        received_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        cwd: std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        session_id: None,
        turn_id: None,
        raw_hook_input: serde_json::json!({}),
        client: ClientInfo {
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            platform: std::env::consts::OS.to_string(),
        },
    };
    let resp = send(
        &req,
        &ClientConfig {
            request_timeout: Duration::from_millis(120),
            poll_interval: Duration::from_millis(5),
            ..Default::default()
        },
    );
    if resp.status == Status::Ok {
        "reachable".into()
    } else {
        "unreachable".into()
    }
}

fn run_cli_capture(args: &[&str], allow_empty: bool) -> Result<String, String> {
    let exe = cli_executable()?;
    let output = hidden_command(&exe)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    let mut text = String::new();
    text.push_str(String::from_utf8_lossy(&output.stdout).trim());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push_str(" | ");
        }
        text.push_str(stderr);
    }
    if !output.status.success() {
        return Err(if text.is_empty() {
            format!("ai-handoff {:?} failed", args)
        } else {
            text
        });
    }
    if text.is_empty() && !allow_empty {
        Ok("ok".into())
    } else {
        Ok(text)
    }
}

fn cli_executable() -> Result<PathBuf, String> {
    let installed = paths::home().join("bin").join(if cfg!(windows) {
        "ai-handoff.exe"
    } else {
        "ai-handoff"
    });
    if installed.is_file() {
        return Ok(installed);
    }

    if let Ok(current) = std::env::current_exe() {
        let sibling = current.with_file_name(if cfg!(windows) {
            "ai-handoff.exe"
        } else {
            "ai-handoff"
        });
        if sibling.is_file() {
            return Ok(sibling);
        }
    }

    let snapshot = dashboard::dashboard_snapshot();
    repair_executable(&snapshot)
}

fn hidden_command<P: AsRef<OsStr>>(program: P) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn open_target(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = {
        let mut command = hidden_command("explorer");
        command.arg(target);
        command
    };

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = hidden_command("open");
        command.arg(target);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = hidden_command("xdg-open");
        command.arg(target);
        command
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn open_file_with_picker(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        hidden_command("rundll32.exe")
            .arg("shell32.dll,OpenAs_RunDLL")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(windows))]
    {
        open_target(&path.to_string_lossy())?;
    }
    Ok(())
}

fn ensure_windows_search_shortcuts() {
    #[cfg(windows)]
    {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return;
        };
        let Ok(target) = std::env::current_exe() else {
            return;
        };
        let dir = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("AI Handoff");
        let script = format!(
            "$dir = {dir}\n\
             $target = {target}\n\
             New-Item -ItemType Directory -Force -Path $dir | Out-Null\n\
             $shell = New-Object -ComObject WScript.Shell\n\
             foreach ($name in @('AI Handoff', 'aho')) {{\n\
             $shortcut = $shell.CreateShortcut((Join-Path $dir ($name + '.lnk')))\n\
             $shortcut.TargetPath = $target\n\
             $shortcut.WorkingDirectory = Split-Path -Parent $target\n\
             $shortcut.IconLocation = $target + ',0'\n\
             $shortcut.Description = 'AI Handoff'\n\
             $shortcut.Save()\n\
             }}\n",
            dir = ps_single_quote(&dir.to_string_lossy()),
            target = ps_single_quote(&target.to_string_lossy()),
        );
        let _ = hidden_command("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

#[cfg(windows)]
fn ps_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn main() {
    tauri::Builder::default()
        .setup(|_| {
            ensure_windows_search_shortcuts();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_dashboard_snapshot,
            list_capsules,
            read_capsule,
            read_logs,
            get_usage_report,
            get_account_report,
            get_integration_report,
            start_account_login,
            poll_account_login,
            launch_account_slot,
            refresh_account_slot_usage,
            run_repair_action,
            get_theme,
            capture_current_account,
            switch_account_slot,
            delete_account_slot,
            get_config_settings,
            set_config_value,
            reset_config_value,
            toggle_capsule_state,
            set_capsule_state,
            set_capsule_field,
            delete_capsule,
            open_capsule_folder,
            open_capsule_external,
            run_doctor,
            create_checkpoint,
            open_logs_folder,
            reinstall_hooks,
            ensure_daemon_running,
            open_project_github
        ])
        .run(tauri::generate_context!())
        .expect("error while running AI Handoff desktop app");
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_core::capsule::{
        AgentKind, Capsule, Consumption, ConsumptionState, RedactionMeta, Session, Summary,
    };
    use ai_handoff_usage::model::{Source, Tokens, UsageEvent};
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn sample_capsule() -> Capsule {
        Capsule {
            schema_version: 2,
            capsule_id: "cap_1".into(),
            project_id: "proj".into(),
            created_at: "2026-06-30T00:00:00Z".into(),
            source_agent: AgentKind::Codex,
            target_agent: AgentKind::ClaudeCode,
            session: Session::default(),
            summary: Summary {
                goal: "old goal".into(),
                done: vec![],
                remaining: vec![],
                risks: vec![],
            },
            files: vec![],
            next_prompt: None,
            redaction: RedactionMeta {
                applied: false,
                ruleset: "none".into(),
            },
            consumption: Consumption {
                state: ConsumptionState::Pending,
                consumed_by: None,
                consumed_at: None,
            },
        }
    }

    fn usage_event(source: Source, model: &str, day: &str, tokens: u64) -> UsageEvent {
        UsageEvent {
            source,
            project: "C:/repo".into(),
            session: "s1".into(),
            model: model.into(),
            day: day.into(),
            tokens: Tokens {
                input: tokens,
                ..Default::default()
            },
        }
    }

    #[test]
    fn config_rows_can_be_set_and_reset_without_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# keep\n[autostart]\nenabled = true\n").unwrap();

        let rows = config_rows_for(&path).unwrap();
        assert!(rows.iter().any(|row| row.key == "capsule.format"));
        assert!(rows.iter().any(|row| row.key == "capsule.language"
            && row.value == "en"
            && row.category == "language"));
        assert!(rows.iter().any(|row| row.key == "gui_theme.codex_color"));
        assert!(!rows.iter().any(|row| row.key == "theme.codex_color"));

        set_config_at(&path, "capsule.format", "md").unwrap();
        let after_set = std::fs::read_to_string(&path).unwrap();
        assert!(after_set.contains("# keep"));
        assert_eq!(
            config_rows_for(&path)
                .unwrap()
                .into_iter()
                .find(|row| row.key == "capsule.format")
                .unwrap()
                .value,
            "md"
        );

        reset_config_at(&path, "capsule.format").unwrap();
        assert_eq!(
            config_rows_for(&path)
                .unwrap()
                .into_iter()
                .find(|row| row.key == "capsule.format")
                .unwrap()
                .value,
            "json"
        );
    }

    #[test]
    fn rejected_config_edit_leaves_existing_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[statusline]\nshow = false\n").unwrap();

        let err = set_config_at(&path, "theme.selection_fg_color", "white").unwrap_err();

        assert!(err.contains("contrast") || err.contains("invalid value"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[statusline]\nshow = false\n"
        );
    }

    #[test]
    fn capsule_commands_edit_state_and_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap_1.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&sample_capsule()).unwrap()).unwrap();

        let new_state = toggle_capsule_state_at(&path).unwrap();
        set_capsule_field_at(&path, "goal", "new goal").unwrap();
        set_capsule_field_at(&path, "remaining", "wire gui | verify").unwrap();
        let capsule = ai_handoff_core::capsule_codec::read_capsule(&path).unwrap();

        assert_eq!(new_state, "in_progress");
        assert_eq!(capsule.consumption.state, ConsumptionState::InProgress);
        assert_eq!(capsule.summary.goal, "new goal");
        assert_eq!(capsule.summary.remaining, vec!["wire gui", "verify"]);
    }

    #[test]
    fn capsule_state_can_be_set_directly_and_consumed_metadata_is_managed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap_1.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&sample_capsule()).unwrap()).unwrap();

        let consumed = set_capsule_state_at(&path, "consumed").unwrap();
        let capsule = ai_handoff_core::capsule_codec::read_capsule(&path).unwrap();
        assert_eq!(consumed, "consumed");
        assert_eq!(capsule.consumption.state, ConsumptionState::Consumed);
        assert_eq!(
            capsule.consumption.consumed_by.as_deref(),
            Some("ai-handoff-gui")
        );
        assert!(capsule.consumption.consumed_at.is_some());

        let blocked = set_capsule_state_at(&path, "blocked").unwrap();
        let capsule = ai_handoff_core::capsule_codec::read_capsule(&path).unwrap();
        assert_eq!(blocked, "blocked");
        assert_eq!(capsule.consumption.state, ConsumptionState::Blocked);
        assert!(capsule.consumption.consumed_by.is_none());
        assert!(capsule.consumption.consumed_at.is_none());

        assert!(set_capsule_state_at(&path, "bad_state").is_err());
    }

    #[test]
    fn usage_report_has_recent_days_newest_first_and_source_totals() {
        let events = vec![
            usage_event(Source::Claude, "claude-opus-4-8", "2026-06-29", 20),
            usage_event(Source::Codex, "gpt-5.5", "2026-06-30", 10),
            usage_event(Source::Codex, "gpt-5.5", "2026-04-01", 99),
        ];

        let report = usage_report_from_events_for_today(
            &events,
            chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        );

        assert_eq!(report.by_day.len(), 30);
        assert_eq!(report.by_day[0].key, "2026-06-30");
        assert_eq!(report.by_day[1].key, "2026-06-29");
        assert_eq!(report.by_day[0].tokens.total, 10);
        assert_eq!(report.total.tokens.total, 129);
        assert!(report.by_source.iter().any(|group| group.key == "codex"));
    }

    #[test]
    fn account_report_lists_saved_slots_and_active_slot() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let codex_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CODEX_HOME", codex_home.path());

        let live = codex_home.path().join("auth.json");
        std::fs::write(
            &live,
            br#"{"tokens":{"id_token":"x","account_id":"work","access_token":"secret"}}"#,
        )
        .unwrap();
        let codex_slot = account::snapshot_current(Agent::Codex).unwrap();
        let claude_dir = account::slot_dir(Agent::Claude, "dev@example.com");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join(".credentials.json"),
            br#"{"claudeAiOauth":{"accessToken":"secret-token","subscriptionType":"pro"}}"#,
        )
        .unwrap();
        std::fs::write(
            claude_dir.join("account.json"),
            br#"{"schema_version":1,"agent":"claude","label":"dev@example.com","email":"dev@example.com","plan_hint":"pro","source":"test"}"#,
        )
        .unwrap();

        let report = account_report(false);

        assert_eq!(
            report.codex.root,
            home.path()
                .join("accounts")
                .join("codex")
                .display()
                .to_string()
        );
        assert_eq!(report.codex.active.as_deref(), Some(codex_slot.as_str()));
        assert_eq!(report.codex.slots.len(), 1);
        assert!(report.codex.slots[0].active);
        assert_eq!(report.claude.slots.len(), 1);
        assert_eq!(
            report.claude.slots[0].email.as_deref(),
            Some("dev@example.com")
        );
        assert_eq!(report.claude.slots[0].plan.as_deref(), Some("pro"));

        std::fs::write(
            &live,
            br#"{"tokens":{"id_token":"x","account_id":"personal","access_token":"secret2"}}"#,
        )
        .unwrap();
        let report = capture_current_account("codex".into()).unwrap();
        assert_eq!(report.codex.active.as_deref(), Some("personal"));
        assert_eq!(report.codex.slots.len(), 2);

        let report = switch_account_slot("codex".into(), codex_slot.clone()).unwrap();
        assert_eq!(report.codex.active.as_deref(), Some(codex_slot.as_str()));

        let report = delete_account_slot("codex".into(), "personal".into()).unwrap();
        assert_eq!(report.codex.slots.len(), 1);
        assert!(report
            .codex
            .slots
            .iter()
            .all(|slot| slot.label != "personal"));
        assert!(parse_agent("unknown").is_err());

        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CODEX_HOME");
    }

    #[test]
    fn account_cli_specs_use_vendor_programs_and_profile_env() {
        let codex_login = account_login_spec(Agent::Codex, PathBuf::from("C:/tmp/codex"));
        assert_eq!(codex_login.env_var, "CODEX_HOME");
        assert_eq!(codex_login.program, "codex");
        assert_eq!(codex_login.args, vec!["login"]);

        let claude_login = account_login_spec(Agent::Claude, PathBuf::from("C:/tmp/claude"));
        assert_eq!(claude_login.env_var, "CLAUDE_CONFIG_DIR");
        assert_eq!(claude_login.program, "claude");
        assert_eq!(claude_login.args, vec!["auth", "login"]);

        let launch = account_launch_spec(Agent::Claude, "dev@example.com");
        assert_eq!(launch.env_var, "CLAUDE_CONFIG_DIR");
        assert_eq!(launch.program, "claude");
        assert!(launch.home.ends_with("accounts/claude/dev@example.com"));
    }

    #[test]
    fn account_login_poll_captures_claude_credentials_without_config_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", live.path());
        std::fs::write(
            profile.path().join(".credentials.json"),
            br#"{"claudeAiOauth":{"accessToken":"secret-token","subscriptionType":"pro"}}"#,
        )
        .unwrap();

        let poll = poll_account_login_at(Agent::Claude, profile.path()).unwrap();

        assert!(poll.done);
        assert_eq!(poll.label.as_deref(), Some("claude-account"));
        let report = poll.report.expect("account report");
        assert_eq!(report.claude.slots.len(), 1);
        assert_eq!(report.claude.slots[0].label, "claude-account");
        assert!(report.claude.slots[0].active);
        assert_eq!(report.claude.active.as_deref(), Some("claude-account"));
        assert_eq!(
            report.claude.slots[0].source.as_deref(),
            Some("official-cli-login")
        );

        std::env::remove_var("AI_HANDOFF_HOME");
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn slot_usage_report_maps_windows_and_reset_credits() {
        let usage = ai_handoff_tui::account_api::UsageData {
            plan: Some("team".into()),
            five_hour: Some(RateWindow {
                used_percent: 25.0,
                window_minutes: 300,
                resets_at: Some(1),
            }),
            weekly: Some(RateWindow {
                used_percent: 50.0,
                window_minutes: 10080,
                resets_at: Some(2),
            }),
            reset_credits: Some(2),
            reset_credit_details: vec![ai_handoff_tui::account_api::ResetCredit {
                granted_at: "2026-06-01T00:00:00Z".into(),
                expires_at: "2026-07-01T00:00:00Z".into(),
            }],
        };

        let report = slot_usage_report_from_data(usage);

        assert_eq!(report.plan.as_deref(), Some("team"));
        assert_eq!(report.five_hour.unwrap().remaining_percent, 75.0);
        assert_eq!(report.weekly.unwrap().window_minutes, 10080);
        assert_eq!(report.reset_credits, Some(2));
        assert_eq!(
            report.reset_credit_details[0].expires_at,
            "2026-07-01T00:00:00Z"
        );
    }

    #[test]
    fn theme_report_resolves_presets_overrides_and_indexed_colors() {
        let dark = config::set_value(None, "gui_theme.preset", "dark").unwrap();
        let dark = config::parse(&dark).unwrap();
        let report = theme_report_from_config(&dark);
        assert_eq!(report.preset, "dark");
        assert_eq!(report.codex_color, "#BD93F9");
        assert_eq!(report.claude_color, "#FFB86C");
        assert_eq!(report.selection_bg_color, "#44475A");
        assert_eq!(report.app_bg_color, "#282A36");

        let custom = config::set_value(None, "gui_theme.preset", "custom").unwrap();
        let custom = config::set_value(Some(&custom), "gui_theme.codex_color", "42").unwrap();
        let custom = config::set_value(Some(&custom), "gui_theme.claude_color", "orange").unwrap();
        let custom =
            config::set_value(Some(&custom), "gui_theme.focus_border_color", "#123456").unwrap();
        let custom =
            config::set_value(Some(&custom), "gui_theme.selection_bg_color", "white").unwrap();
        let custom =
            config::set_value(Some(&custom), "gui_theme.selection_fg_color", "black").unwrap();
        let custom = config::parse(&custom).unwrap();
        let report = theme_report_from_config(&custom);
        assert_eq!(report.codex_color, "#00D787");
        assert_eq!(report.claude_color, "#FFA500");
        assert_eq!(report.focus_border_color, "#123456");
        assert_eq!(report.selection_bg_color, "#FFFFFF");
        assert_eq!(report.selection_fg_color, "#000000");
    }

    #[test]
    fn theme_report_maps_legacy_dark_tuple_to_dracula() {
        let legacy = config::parse(
            "[gui_theme]\n\
             preset = \"dark\"\n\
             codex_color = \"#C8A7FF\"\n\
             claude_color = \"#FFB05C\"\n\
             focus_border_color = \"#FF9F43\"\n\
             selection_bg_color = \"#FF79C6\"\n\
             selection_fg_color = \"#111318\"\n\
             app_bg_color = \"#0B0D14\"\n\
             sidebar_bg_color = \"#111522\"\n\
             panel_bg_color = \"#191D2A\"\n\
             text_color = \"#F8F8F2\"\n",
        )
        .unwrap();
        let report = theme_report_from_config(&legacy);

        assert_eq!(report.preset, "dark");
        assert_eq!(report.codex_color, "#BD93F9");
        assert_eq!(report.claude_color, "#FFB86C");
        assert_eq!(report.focus_border_color, "#FF79C6");
        assert_eq!(report.selection_bg_color, "#44475A");
        assert_eq!(report.selection_fg_color, "#F8F8F2");
        assert_eq!(report.app_bg_color, "#282A36");
        assert_eq!(report.sidebar_bg_color, "#21222C");
        assert_eq!(report.panel_bg_color, "#282A36");
    }

    fn check(id: &str, status: dashboard::CheckStatus, message: &str) -> dashboard::CheckRow {
        dashboard::CheckRow {
            id: id.into(),
            label: id.into(),
            status,
            message: message.into(),
            path: None,
        }
    }

    fn integration_snapshot_for_test() -> DashboardSnapshot {
        let daemon = check("daemon", dashboard::CheckStatus::Error, "offline");
        let autostart = check("autostart", dashboard::CheckStatus::Warning, "HKCU Run");
        let codex_hooks = check("codex-hooks", dashboard::CheckStatus::Ok, "installed");
        let codex_config = check(
            "codex-config",
            dashboard::CheckStatus::Warning,
            "trust needed",
        );
        let claude_settings = check("claude-settings", dashboard::CheckStatus::Ok, "installed");
        let ipc = check("ipc", dashboard::CheckStatus::Ok, "present");
        let store = check("store", dashboard::CheckStatus::Missing, "missing");
        let duplicate = check(
            "duplicate-0",
            dashboard::CheckStatus::Warning,
            "legacy hook",
        );
        let checks = vec![
            daemon.clone(),
            autostart.clone(),
            codex_hooks.clone(),
            codex_config.clone(),
            claude_settings.clone(),
            ipc.clone(),
            store.clone(),
            duplicate.clone(),
        ];
        DashboardSnapshot {
            paths: dashboard::DashboardPaths {
                ai_home: "C:/home".into(),
                ipc: "C:/home/ipc".into(),
                store: "C:/home/store".into(),
                logs: "C:/home/logs".into(),
                install_state: "C:/home/install.json".into(),
                codex_hooks: "C:/Users/PC/.codex/hooks.json".into(),
                codex_config: "C:/Users/PC/.codex/config.toml".into(),
                claude_settings: "C:/Users/PC/.claude/settings.json".into(),
            },
            install_state: dashboard::InstallSummary {
                status: dashboard::CheckStatus::Ok,
                version: 2,
                installed_at: "2026-06-30T00:00:00Z".into(),
                autostart: "HKCU Run: AI Handoff".into(),
                launcher: Some("C:/home/bin/ai-handoff.exe".into()),
            },
            daemon,
            autostart,
            codex_hooks,
            codex_config,
            claude_settings,
            ipc,
            store,
            duplicates: vec![duplicate],
            capsules: dashboard::CapsuleList {
                items: vec![],
                pending_count: 0,
                skipped: 0,
            },
            checks,
        }
    }

    fn account_report_for_test() -> AccountReport {
        AccountReport {
            codex: AccountAgentReport {
                agent: "codex".into(),
                root: "C:/home/accounts/codex".into(),
                active: None,
                plan: None,
                five_hour: None,
                weekly: None,
                slots: vec![AccountSlotRow {
                    label: "work".into(),
                    email: None,
                    plan: None,
                    account_id: None,
                    source: None,
                    created_at: None,
                    active: true,
                    path: "C:/home/accounts/codex/work".into(),
                }],
            },
            claude: AccountAgentReport {
                agent: "claude".into(),
                root: "C:/home/accounts/claude".into(),
                active: None,
                plan: None,
                five_hour: None,
                weekly: None,
                slots: vec![],
            },
        }
    }

    #[test]
    fn integration_report_summarizes_doctor_and_dedupes_repair_actions() {
        let report = integration_report_from_parts(
            integration_snapshot_for_test(),
            account_report_for_test(),
            "unreachable".into(),
            17,
        );

        assert_eq!(report.doctor.daemon, "unreachable");
        assert_eq!(report.doctor.ok, 3);
        assert_eq!(report.doctor.warn, 3);
        assert_eq!(report.doctor.fail, 2);
        assert_eq!(report.doctor.codex_accounts, 1);
        assert_eq!(report.doctor.claude_accounts, 0);
        let ids = report
            .repairs
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "start_daemon",
                "autostart_on",
                "install_plugin",
                "manual_legacy_cleanup",
                "manual_codex_trust",
                "run_doctor"
            ]
        );
        assert_eq!(
            report.repairs[2].command,
            Some(vec!["install".to_string(), "--yes".to_string()])
        );
        assert!(report.repairs[2].requires_confirm);
        assert!(report.repairs[3].manual);
    }
}
