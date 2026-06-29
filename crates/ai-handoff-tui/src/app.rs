//! The ratatui application: state, key handling, and drawing for the
//! Overview / Capsule / Account / Settings tabs.
//!
//! `on_key` is kept independent of the terminal (it only mutates state and, on
//! a Settings save, writes config) so the interaction logic is unit-testable
//! without a TTY. The draw + event loop are the thin, untested shell.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Points},
        Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Table, Tabs, Wrap,
    },
    DefaultTerminal, Frame,
};

use rust_i18n::t;

use ai_handoff_core::account::{self, Agent, RateWindow};
use ai_handoff_core::config::{self, Config, KeyKind};
use ai_handoff_core::dashboard::{CheckStatus, DashboardSnapshot};
use ai_handoff_usage::Dimension;

use crate::account_api::ResetCredit;
use crate::capsule_ops;
use crate::edit::{self, EditAction};
use crate::viewmodel::{
    capsule_tree, health_rows, settings_rows, CapsuleAgent, HealthRow, SettingRow, UsageView,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Capsule,
    Usage,
    Account,
    Integration,
    Settings,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::Overview,
        Tab::Capsule,
        Tab::Usage,
        Tab::Account,
        Tab::Integration,
        Tab::Settings,
    ];
    /// Translation key for the tab's title (resolved at render time via `t!`).
    fn title_key(self) -> &'static str {
        match self {
            Tab::Overview => "tab.overview",
            Tab::Capsule => "tab.capsule",
            Tab::Usage => "tab.usage",
            Tab::Account => "tab.account",
            Tab::Integration => "tab.integration",
            Tab::Settings => "tab.settings",
        }
    }
    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
    fn next(self) -> Tab {
        Tab::ALL[(self.index() + 1) % Tab::ALL.len()]
    }
    fn prev(self) -> Tab {
        Tab::ALL[(self.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }
}

/// The default status-bar hint, in the active language.
fn default_hint() -> String {
    t!("hint.default").into_owned()
}

/// A one-line description of a setting key (shown in the status bar while
/// browsing Settings), or empty for an unknown key.
fn setting_desc(key: &str) -> String {
    let desc_key = match key {
        "triggers.five_hour.enabled" => "setting.five_hour_enabled",
        "triggers.five_hour.threshold_percent" => "setting.threshold",
        "triggers.five_hour.mode" => "setting.mode",
        "triggers.five_hour.burn_rate.enabled" => "setting.burn_enabled",
        "triggers.five_hour.burn_rate.runway_minutes" => "setting.runway",
        "autostart.enabled" => "setting.autostart",
        "statusline.show" => "setting.statusline",
        "language" => "setting.language",
        "capsule.format" => "setting.capsule_format",
        "capsule.next_prompt_max_items" => "setting.capsule_next_prompt_max_items",
        "capsule.remaining_max_items" => "setting.capsule_remaining_max_items",
        "capsule.done_max_items" => "setting.capsule_done_max_items",
        "capsule.risks_max_items" => "setting.capsule_risks_max_items",
        "theme.preset" => "setting.theme_preset",
        "theme.codex_color" => "setting.theme_codex_color",
        "theme.claude_color" => "setting.theme_claude_color",
        "theme.focus_border_color" => "setting.theme_focus_border_color",
        "theme.selection_bg_color" => "setting.theme_selection_bg_color",
        "theme.selection_fg_color" => "setting.theme_selection_fg_color",
        _ => return String::new(),
    };
    t!(desc_key).into_owned()
}

/// The quit-confirmation hint, in the active language.
fn quit_hint() -> String {
    t!("hint.quit").into_owned()
}

/// Claude = orange, Codex = light purple (agent labels + usage visuals).
const CLAUDE_COLOR: Color = Color::Rgb(230, 140, 30);
const CODEX_LABEL_COLOR: Color = Color::Rgb(185, 150, 235);
const DEFAULT_FOCUS_BORDER_COLOR: Color = Color::Rgb(255, 165, 0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TuiTheme {
    codex: Color,
    claude: Color,
    focus_border: Color,
    selection_bg: Color,
    selection_fg: Color,
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self {
            codex: CODEX_LABEL_COLOR,
            claude: CLAUDE_COLOR,
            focus_border: DEFAULT_FOCUS_BORDER_COLOR,
            selection_bg: Color::Cyan,
            selection_fg: Color::Black,
        }
    }
}

impl TuiTheme {
    fn from_config(cfg: &Config) -> Self {
        let base_cfg = config::theme_config_for_preset(cfg.theme.preset);
        let default_cfg = config::ThemeConfig::default();
        let mut theme = Self {
            codex: tui_color(base_cfg.codex_color.as_str()).unwrap_or(Self::default().codex),
            claude: tui_color(base_cfg.claude_color.as_str()).unwrap_or(Self::default().claude),
            focus_border: tui_color(base_cfg.focus_border_color.as_str())
                .unwrap_or(Self::default().focus_border),
            selection_bg: tui_color(base_cfg.selection_bg_color.as_str())
                .unwrap_or(Self::default().selection_bg),
            selection_fg: tui_color(base_cfg.selection_fg_color.as_str())
                .unwrap_or(Self::default().selection_fg),
        };
        let custom = cfg.theme.preset == config::ThemePreset::Custom;
        if custom || cfg.theme.codex_color.as_str() != default_cfg.codex_color.as_str() {
            theme.codex = tui_color(cfg.theme.codex_color.as_str()).unwrap_or(theme.codex);
        }
        if custom || cfg.theme.claude_color.as_str() != default_cfg.claude_color.as_str() {
            theme.claude = tui_color(cfg.theme.claude_color.as_str()).unwrap_or(theme.claude);
        }
        if custom
            || cfg.theme.focus_border_color.as_str() != default_cfg.focus_border_color.as_str()
        {
            theme.focus_border =
                tui_color(cfg.theme.focus_border_color.as_str()).unwrap_or(theme.focus_border);
        }
        if custom
            || cfg.theme.selection_bg_color.as_str() != default_cfg.selection_bg_color.as_str()
        {
            theme.selection_bg =
                tui_color(cfg.theme.selection_bg_color.as_str()).unwrap_or(theme.selection_bg);
        }
        if custom
            || cfg.theme.selection_fg_color.as_str() != default_cfg.selection_fg_color.as_str()
        {
            theme.selection_fg =
                tui_color(cfg.theme.selection_fg_color.as_str()).unwrap_or(theme.selection_fg);
        }
        theme
    }
}

fn tui_color(raw: &str) -> Option<Color> {
    let value = raw.trim();
    if let Some((r, g, b)) = config::ColorSpec::parse(value)
        .ok()
        .and_then(|spec| spec.rgb())
    {
        return Some(Color::Rgb(r, g, b));
    }
    value.parse::<u8>().ok().map(Color::Indexed)
}

/// One visible line in the Capsule tab's tree (agent → project → capsule).
struct CapRow {
    indent: usize,
    label: String,
    target: CapTarget,
}

/// What a `CapRow` points at, by index into the agent → project → capsule tree.
#[derive(Clone, Copy)]
enum CapTarget {
    Agent(usize),
    Project(usize, usize),
    Capsule(usize, usize, usize),
}

/// Where the focus sits inside the Capsule tab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CapFocus {
    /// The left (3/10) agent → project → capsule tree.
    Tree,
    /// The right (7/10) detail pane: action bar on top, body below.
    Detail,
    /// Editing the selected capsule's goal in place.
    Editing,
}

/// Cached detail for the selected capsule: the parsed capsule (when it is a
/// valid v2 capsule) and the raw file text as a fallback.
struct CapDetail {
    path: String,
    parsed: Option<ai_handoff_core::capsule::Capsule>,
    raw: String,
}

/// Where the focus sits inside the Account tab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AccFocus {
    /// The left (3/10) Codex / Claude account tree.
    Tree,
    /// The right (7/10) detail pane: switch / delete + plan & limits.
    Detail,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum UsageFocus {
    Chart,
    Details,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum UsageViewMode {
    Summary,
    Day,
    Project,
    Model,
    Source,
}

impl UsageViewMode {
    fn next(self) -> Self {
        match self {
            Self::Summary => Self::Day,
            Self::Day => Self::Project,
            Self::Project => Self::Model,
            Self::Model => Self::Source,
            Self::Source => Self::Summary,
        }
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::Summary => "usage.mode.summary",
            Self::Day => "usage.mode.day",
            Self::Project => "usage.mode.project",
            Self::Model => "usage.mode.model",
            Self::Source => "usage.mode.source",
        }
    }

    fn dimension(self) -> Option<Dimension> {
        match self {
            Self::Summary => None,
            Self::Day => Some(Dimension::Day),
            Self::Project => Some(Dimension::Project),
            Self::Model => Some(Dimension::Model),
            Self::Source => Some(Dimension::Source),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IntegrationFocus {
    Status,
    Repair,
    Hooks,
    Diagnostics,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IntegrationPage {
    Home,
    Detail,
    RepairCenter,
    DoctorRun,
    Logs,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SettingsFocus {
    Category,
    Detail,
}

/// What an account row points at.
#[derive(Clone, Copy)]
enum AccTarget {
    /// The agent header line (selecting it shows that agent's live status).
    Header(Agent),
    /// A saved account snapshot (index into that agent's slots).
    Slot(Agent, usize),
    /// The "+ capture current" action line.
    Add(Agent),
}

/// One visible line in the Account tab's tree.
struct AccRow {
    indent: usize,
    label: String,
    target: AccTarget,
}

/// A deferred action queued by a keypress and started by the event loop (each
/// opens its own window, so the TUI is never suspended).
#[derive(Clone, PartialEq, Eq, Debug)]
enum Pending {
    /// `codex login` / `claude auth login` in a new window, then capture.
    AddAccount(Agent),
    /// Launch the agent under a saved slot's profile home, in a new window.
    Launch(Agent, String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RepairActionKind {
    RunDoctor,
    InstallPlugin,
    StartDaemon,
    AutostartOn,
    ManualLegacyCleanup,
    ManualCodexTrust,
}

impl RepairActionKind {
    fn label_key(self) -> &'static str {
        match self {
            Self::RunDoctor => "integration.repair.run_doctor",
            Self::InstallPlugin => "integration.repair.install_plugin",
            Self::StartDaemon => "integration.repair.start_daemon",
            Self::AutostartOn => "integration.repair.autostart_on",
            Self::ManualLegacyCleanup => "integration.repair.disable_legacy",
            Self::ManualCodexTrust => "integration.repair.open_trust_guide",
        }
    }

    fn detail_key(self) -> &'static str {
        match self {
            Self::RunDoctor => "integration.repair_detail.run_doctor",
            Self::InstallPlugin => "integration.repair_detail.install_plugin",
            Self::StartDaemon => "integration.repair_detail.start_daemon",
            Self::AutostartOn => "integration.repair_detail.autostart_on",
            Self::ManualLegacyCleanup => "integration.repair_detail.disable_legacy",
            Self::ManualCodexTrust => "integration.repair_detail.open_trust_guide",
        }
    }

    fn requires_confirm(self) -> bool {
        matches!(
            self,
            Self::InstallPlugin | Self::StartDaemon | Self::AutostartOn
        )
    }

    fn is_manual(self) -> bool {
        matches!(self, Self::ManualLegacyCleanup | Self::ManualCodexTrust)
    }
}

/// An in-flight add: a login window is open and we poll its temp home for the
/// credential to appear.
struct LoginPoll {
    agent: Agent,
    home: PathBuf,
    deadline: std::time::Instant,
}

/// Per-account usage fetched from the backend (5h / weekly / credits). Fetched
/// in a background thread so navigating accounts never blocks.
#[derive(Clone, PartialEq)]
enum UsageState {
    Loading,
    Loaded(crate::account_api::UsageData),
    Error(String),
}

/// One agent's account picture: who is signed in, their live limits, and the
/// saved snapshots in the pool.
#[derive(Default)]
struct AgentAccount {
    /// Live usage for the *active* account (Claude only — Codex usage is fetched
    /// per-account from the backend instead).
    status: Option<account::AccountStatus>,
    slots: Vec<account::AccountSlot>,
}

/// Both agents' account data for the Account tab.
#[derive(Default)]
struct AccountData {
    codex: AgentAccount,
    claude: AgentAccount,
}

#[derive(Default)]
struct OverviewAgentLimits {
    five_hour: Option<RateWindow>,
    weekly: Option<RateWindow>,
    note: Option<String>,
}

impl AccountData {
    /// Scan the live system (rollout limits, auth identity, pool snapshots).
    fn load_live() -> Self {
        AccountData {
            codex: AgentAccount {
                status: None, // Codex usage is fetched per-account from the backend.
                slots: account::list_slots(Agent::Codex),
            },
            claude: AgentAccount {
                status: account::claude_status(),
                slots: account::list_slots(Agent::Claude),
            },
        }
    }

    fn agent(&self, agent: Agent) -> &AgentAccount {
        match agent {
            Agent::Codex => &self.codex,
            Agent::Claude => &self.claude,
        }
    }
}

pub struct App {
    pub tab: Tab,
    /// Whether the focus is inside the current tab's content (vs. the tab bar).
    /// A top tab only descends into its content on ↓/Space/Enter.
    focus_content: bool,
    snapshot: DashboardSnapshot,
    usage: UsageView,
    settings: Vec<SettingRow>,
    settings_idx: usize,
    settings_category_idx: usize,
    settings_focus: SettingsFocus,
    settings_search: Option<String>,
    settings_search_editing: bool,
    settings_edit_buf: Option<String>,
    theme: TuiTheme,
    config_path: PathBuf,
    usage_focus: UsageFocus,
    usage_mode: UsageViewMode,
    integration_focus: IntegrationFocus,
    integration_page: IntegrationPage,
    repair_sel: usize,
    repair_confirm: bool,
    integration_output: Vec<String>,
    integration_logs: Vec<String>,
    // --- Account tab state ---
    account: AccountData,
    /// Whether focus is on the account tree or the detail pane.
    acc_focus: AccFocus,
    /// Selected row in the flattened account tree.
    acc_sel: usize,
    /// A delete needs a second confirm press; armed here.
    acc_confirm_delete: bool,
    /// Per-account usage cache, keyed by `usage_key(agent, label)`.
    acc_usage: std::collections::HashMap<String, UsageState>,
    /// Background-fetch result channel (key, usage result).
    usage_tx: std::sync::mpsc::Sender<(String, Result<crate::account_api::UsageData, String>)>,
    usage_rx: std::sync::mpsc::Receiver<(String, Result<crate::account_api::UsageData, String>)>,
    /// An action queued by a keypress (add / launch), started next loop.
    pending: Option<Pending>,
    /// An open login window being polled for its captured credential.
    login_poll: Option<LoginPoll>,
    // --- Capsule tab state ---
    cap_tree: Vec<CapsuleAgent>,
    cap_expanded_agents: HashSet<usize>,
    cap_expanded_projects: HashSet<(usize, usize)>,
    cap_sel: usize,
    /// Whether focus is on the tree, the detail pane, or the field editor.
    cap_focus: CapFocus,
    /// Which editable field is selected in the detail pane (index into CAP_FIELDS).
    cap_field: usize,
    /// A delete needs a second confirm press; armed here.
    cap_confirm_delete: bool,
    /// The working buffer while editing a capsule field.
    cap_edit_buf: String,
    /// Cached parsed/raw detail of the currently-selected capsule.
    cap_detail: Option<CapDetail>,
    status: String,
    should_quit: bool,
    /// Armed on a top tab by a first q/Esc; a second one quits.
    confirm_quit: bool,
}

/// The capsule fields the detail pane lets you edit, in display order.
const CAP_FIELDS: [capsule_ops::CapField; 5] = [
    capsule_ops::CapField::Goal,
    capsule_ops::CapField::NextPrompt,
    capsule_ops::CapField::Remaining,
    capsule_ops::CapField::Done,
    capsule_ops::CapField::Risks,
];

struct SettingCategory {
    key: &'static str,
    desc_key: &'static str,
}

const SETTING_CATEGORIES: [SettingCategory; 9] = [
    SettingCategory {
        key: "settings.category.all",
        desc_key: "settings.category_desc.all",
    },
    SettingCategory {
        key: "settings.category.automation",
        desc_key: "settings.category_desc.automation",
    },
    SettingCategory {
        key: "settings.category.triggers",
        desc_key: "settings.category_desc.triggers",
    },
    SettingCategory {
        key: "settings.category.capsule",
        desc_key: "settings.category_desc.capsule",
    },
    SettingCategory {
        key: "settings.category.paths",
        desc_key: "settings.category_desc.paths",
    },
    SettingCategory {
        key: "settings.category.display",
        desc_key: "settings.category_desc.display",
    },
    SettingCategory {
        key: "settings.category.security",
        desc_key: "settings.category_desc.security",
    },
    SettingCategory {
        key: "settings.category.agents",
        desc_key: "settings.category_desc.agents",
    },
    SettingCategory {
        key: "settings.category.advanced",
        desc_key: "settings.category_desc.advanced",
    },
];

impl App {
    /// Build the app by scanning the live system (logs, config, health).
    pub fn load() -> Self {
        let snapshot = ai_handoff_core::dashboard::dashboard_snapshot();
        let usage = UsageView::from_events(&ai_handoff_usage::scan_default());
        let cfg = ai_handoff_core::config::load();
        let config_path = ai_handoff_core::paths::config_path();
        let mut app = App::new(snapshot, usage, settings_rows(&cfg), config_path);
        app.apply_theme_config(&cfg);
        app.account = AccountData::load_live();
        app
    }

    pub fn new(
        snapshot: DashboardSnapshot,
        usage: UsageView,
        settings: Vec<SettingRow>,
        config_path: PathBuf,
    ) -> Self {
        let cap_tree = capsule_tree(&snapshot.capsules);
        // Start with every agent expanded so the projects are visible at a glance.
        let cap_expanded_agents = (0..cap_tree.len()).collect();
        let (usage_tx, usage_rx) = std::sync::mpsc::channel();
        App {
            tab: Tab::Overview,
            focus_content: false,
            snapshot,
            usage,
            settings,
            settings_idx: 0,
            settings_category_idx: 0,
            settings_focus: SettingsFocus::Category,
            settings_search: None,
            settings_search_editing: false,
            settings_edit_buf: None,
            theme: TuiTheme::default(),
            config_path,
            usage_focus: UsageFocus::Chart,
            usage_mode: UsageViewMode::Summary,
            integration_focus: IntegrationFocus::Status,
            integration_page: IntegrationPage::Home,
            repair_sel: 0,
            repair_confirm: false,
            integration_output: Vec::new(),
            integration_logs: Vec::new(),
            account: AccountData::default(),
            acc_focus: AccFocus::Tree,
            acc_sel: 0,
            acc_confirm_delete: false,
            acc_usage: std::collections::HashMap::new(),
            usage_tx,
            usage_rx,
            pending: None,
            login_poll: None,
            cap_tree,
            cap_expanded_agents,
            cap_expanded_projects: HashSet::new(),
            cap_sel: 0,
            cap_focus: CapFocus::Tree,
            cap_field: 0,
            cap_confirm_delete: false,
            cap_edit_buf: String::new(),
            cap_detail: None,
            status: default_hint(),
            should_quit: false,
            confirm_quit: false,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    fn apply_theme_config(&mut self, cfg: &Config) {
        self.theme = TuiTheme::from_config(cfg);
    }

    fn refresh_theme_from_disk(&mut self) {
        let cfg = config::load_from(&self.config_path);
        self.apply_theme_config(&cfg);
    }

    fn selection_style(&self) -> Style {
        Style::default()
            .fg(self.theme.selection_fg)
            .bg(self.theme.selection_bg)
    }

    fn focus_block(&self, title: impl Into<String>, focused: bool) -> Block<'static> {
        focus_block_with_color(title, focused, self.theme.focus_border)
    }

    fn action_style(&self, focused: bool) -> Style {
        if focused {
            self.selection_style()
        } else {
            Style::default().fg(Color::Gray)
        }
    }

    fn agent_color(&self, agent: Agent) -> Color {
        match agent {
            Agent::Codex => self.theme.codex,
            Agent::Claude => self.theme.claude,
        }
    }

    fn agent_label_color(&self, name: &str) -> Option<Color> {
        let lower = name.to_ascii_lowercase();
        if lower.contains("codex") {
            Some(self.theme.codex)
        } else if lower.contains("claude") {
            Some(self.theme.claude)
        } else {
            None
        }
    }

    /// The event loop. Returns when the user quits.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.should_quit {
            if self.tab == Tab::Overview {
                self.ensure_overview_limit_usage();
            }
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.on_key(key);
                    }
                }
            }
            // A key may have queued an action (login / launch). Each opens its
            // own window, so the TUI keeps running here.
            if let Some(pending) = self.pending.take() {
                self.handle_pending(pending);
            }
            self.poll_login();
            // Apply any background per-account usage fetches that finished.
            while let Ok((key, res)) = self.usage_rx.try_recv() {
                let state = match res {
                    Ok(u) => UsageState::Loaded(u),
                    Err(e) => UsageState::Error(e),
                };
                self.acc_usage.insert(key, state);
            }
        }
        Ok(())
    }

    /// Start a queued action: launch opens a window immediately; add opens a
    /// login window and arms a poll for the captured credential.
    fn handle_pending(&mut self, pending: Pending) {
        match pending {
            Pending::Launch(agent, label) => {
                self.status = match crate::account_login::spawn_launch_window(agent, &label) {
                    Ok(()) => t!("status.account_launched", label = label).into_owned(),
                    Err(e) => t!("status.account_launch_failed", err = e).into_owned(),
                };
            }
            Pending::AddAccount(agent) => match crate::account_login::spawn_add_window(agent) {
                Ok(home) => {
                    self.login_poll = Some(LoginPoll {
                        agent,
                        home,
                        deadline: std::time::Instant::now() + Duration::from_secs(300),
                    });
                    self.status = t!("status.account_login_window").into_owned();
                }
                Err(e) => {
                    self.status = t!("status.account_capture_failed", err = e).into_owned();
                }
            },
        }
    }

    /// While a login window is open, capture the credential once it lands (or
    /// give up after the deadline).
    fn poll_login(&mut self) {
        let Some(poll) = self.login_poll.as_ref() else {
            return;
        };
        if account::login_complete(poll.agent, &poll.home) {
            let (agent, home) = (poll.agent, poll.home.clone());
            self.login_poll = None;
            let status = match account::capture_login(agent, &home, "official-cli-login") {
                Ok(label) => t!("status.account_captured", label = label).into_owned(),
                Err(e) => t!("status.account_capture_failed", err = e.to_string()).into_owned(),
            };
            let _ = std::fs::remove_dir_all(&home);
            self.reload_account();
            self.status = status;
        } else if std::time::Instant::now() > poll.deadline {
            let home = poll.home.clone();
            self.login_poll = None;
            let _ = std::fs::remove_dir_all(&home);
            self.status = t!("status.account_login_timeout").into_owned();
        }
    }

    /// Handle one keypress. Pure except for a config file write on Settings edit.
    pub fn on_key(&mut self, key: KeyEvent) {
        // While editing a capsule goal, the editor owns every key (so typing
        // 'q', a digit, or Tab inserts text instead of navigating).
        if self.tab == Tab::Capsule && self.cap_focus == CapFocus::Editing {
            self.cap_editing_key(key);
            return;
        }
        if self.tab == Tab::Settings && self.settings_edit_buf.is_some() {
            self.settings_editing_key(key);
            return;
        }
        if self.tab == Tab::Settings && self.settings_search_editing {
            self.settings_search_key(key);
            return;
        }
        // q/Esc backs out one level: content -> tab bar, then tab history, then
        // a quit confirmation at the root.
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
            self.on_back();
            return;
        }
        // Any other key disarms a pending quit confirmation.
        if self.confirm_quit {
            self.confirm_quit = false;
            self.status = default_hint();
        }
        // Tab / Shift-Tab / number keys switch tabs from either level and land
        // back on the tab bar (so each tab is re-entered explicitly).
        match key.code {
            KeyCode::Tab => return self.goto(self.tab.next()),
            KeyCode::BackTab => return self.goto(self.tab.prev()),
            KeyCode::Char('1') => return self.goto(Tab::Overview),
            KeyCode::Char('2') => return self.goto(Tab::Capsule),
            KeyCode::Char('3') => return self.goto(Tab::Usage),
            KeyCode::Char('4') => return self.goto(Tab::Account),
            KeyCode::Char('5') => return self.goto(Tab::Integration),
            KeyCode::Char('6') => return self.goto(Tab::Settings),
            _ => {}
        }
        if !self.focus_content {
            // Tab-bar level: ←/→ move between tabs; ↓/Space/Enter descend.
            match key.code {
                KeyCode::Left => self.goto(self.tab.prev()),
                KeyCode::Right => self.goto(self.tab.next()),
                KeyCode::Down | KeyCode::Char(' ') | KeyCode::Enter => self.enter_content(),
                _ => {}
            }
            return;
        }
        // Content level: per-tab keys.
        match self.tab {
            Tab::Overview => {}
            Tab::Capsule => self.on_capsule_key(key),
            Tab::Usage => self.on_usage_key(key),
            Tab::Account => self.on_account_key(key),
            Tab::Integration => self.on_integration_key(key),
            Tab::Settings => self.on_settings_key(key),
        }
    }

    /// Switch tabs. Always lands on the tab bar (content focus is dropped).
    fn goto(&mut self, target: Tab) {
        self.tab = target;
        self.focus_content = false;
        self.confirm_quit = false;
        self.status = default_hint();
    }

    /// Descend from the tab bar into the current tab's content.
    fn enter_content(&mut self) {
        self.focus_content = true;
        self.status = match self.tab {
            Tab::Overview => default_hint(),
            Tab::Capsule => {
                self.cap_focus = CapFocus::Tree;
                self.cap_confirm_delete = false;
                self.cap_load_content();
                t!("hint.capsule_tree").into_owned()
            }
            Tab::Usage => {
                self.usage_focus = UsageFocus::Chart;
                t!("hint.usage").into_owned()
            }
            Tab::Account => {
                self.acc_focus = AccFocus::Tree;
                self.acc_confirm_delete = false;
                t!("hint.account_tree").into_owned()
            }
            Tab::Integration => {
                self.integration_focus = IntegrationFocus::Status;
                self.integration_page = IntegrationPage::Home;
                self.repair_confirm = false;
                t!("hint.integration").into_owned()
            }
            Tab::Settings => {
                self.settings_focus = SettingsFocus::Category;
                self.settings_category_idx = 0;
                t!("hint.settings").into_owned()
            }
        };
    }

    /// q/Esc: inside a tab's content, just leave it (back to the tab bar). On a
    /// top tab, arm a quit confirmation; a second q/Esc actually quits.
    fn on_back(&mut self) {
        if self.focus_content
            && self.tab == Tab::Integration
            && self.integration_page != IntegrationPage::Home
        {
            if self.integration_page == IntegrationPage::RepairCenter && self.repair_confirm {
                self.repair_confirm = false;
                self.status = t!("status.repair_cancelled").into_owned();
                return;
            }
            self.integration_page = IntegrationPage::Home;
            self.repair_confirm = false;
            self.status = t!("hint.integration").into_owned();
            return;
        }
        if self.focus_content {
            if self.tab == Tab::Settings {
                if self.settings_edit_buf.take().is_some() {
                    self.status = t!("status.settings_edit_cancelled").into_owned();
                    return;
                }
                if self.settings_search.take().is_some() {
                    self.settings_search_editing = false;
                    self.status = t!("hint.settings").into_owned();
                    return;
                }
            }
            self.focus_content = false;
            self.confirm_quit = false;
            self.status = default_hint();
            return;
        }
        if self.confirm_quit {
            self.should_quit = true;
        } else {
            self.confirm_quit = true;
            self.status = quit_hint();
        }
    }

    fn on_usage_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('g') {
            self.usage_mode = self.usage_mode.next();
            self.status = t!(
                "status.usage_mode",
                mode = t!(self.usage_mode.label_key()).into_owned()
            )
            .into_owned();
            return;
        }
        match (self.usage_focus, key.code) {
            (UsageFocus::Chart, KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ')) => {
                self.usage_focus = UsageFocus::Details;
            }
            (UsageFocus::Details, KeyCode::Left) => {
                self.usage_focus = UsageFocus::Chart;
            }
            _ => {}
        }
    }

    fn on_integration_key(&mut self, key: KeyEvent) {
        if self.integration_page != IntegrationPage::Home {
            self.on_integration_page_key(key);
            return;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.integration_page = IntegrationPage::Detail;
                self.status = t!("hint.integration_detail").into_owned();
            }
            KeyCode::Char('r') => {
                self.integration_page = IntegrationPage::RepairCenter;
                self.repair_sel = 0;
                self.repair_confirm = false;
                self.status = t!("hint.integration_repair").into_owned();
            }
            KeyCode::Char('d') => self.run_integration_doctor(),
            KeyCode::Char('l') => self.open_integration_logs(),
            KeyCode::Right if self.integration_focus == IntegrationFocus::Status => {
                self.integration_focus = IntegrationFocus::Repair;
            }
            KeyCode::Left if self.integration_focus == IntegrationFocus::Repair => {
                self.integration_focus = IntegrationFocus::Status;
            }
            KeyCode::Left => self.integration_focus = IntegrationFocus::Status,
            KeyCode::Down | KeyCode::Char('j') => {
                self.integration_focus = match self.integration_focus {
                    IntegrationFocus::Status | IntegrationFocus::Repair => IntegrationFocus::Hooks,
                    IntegrationFocus::Hooks => IntegrationFocus::Diagnostics,
                    IntegrationFocus::Diagnostics => IntegrationFocus::Diagnostics,
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.integration_focus = match self.integration_focus {
                    IntegrationFocus::Diagnostics => IntegrationFocus::Hooks,
                    IntegrationFocus::Hooks => IntegrationFocus::Status,
                    IntegrationFocus::Status | IntegrationFocus::Repair => IntegrationFocus::Status,
                };
            }
            _ => {}
        }
    }

    fn on_integration_page_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('d') => self.run_integration_doctor(),
            KeyCode::Char('l') => self.open_integration_logs(),
            KeyCode::Char('r') => {
                self.integration_page = IntegrationPage::RepairCenter;
                self.repair_confirm = false;
                self.status = t!("hint.integration_repair").into_owned();
            }
            KeyCode::Up | KeyCode::Char('k')
                if self.integration_page == IntegrationPage::RepairCenter =>
            {
                if self.repair_sel > 0 {
                    self.repair_sel -= 1;
                }
                self.repair_confirm = false;
            }
            KeyCode::Down | KeyCode::Char('j')
                if self.integration_page == IntegrationPage::RepairCenter =>
            {
                let n = self.recommended_repair_actions().len();
                if self.repair_sel + 1 < n {
                    self.repair_sel += 1;
                }
                self.repair_confirm = false;
            }
            KeyCode::Enter | KeyCode::Char('y')
                if self.integration_page == IntegrationPage::RepairCenter =>
            {
                self.activate_repair_selection();
            }
            KeyCode::Char('n') if self.integration_page == IntegrationPage::RepairCenter => {
                self.repair_confirm = false;
                self.status = t!("status.repair_cancelled").into_owned();
            }
            _ => {}
        }
    }

    fn on_settings_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('/') {
            self.settings_search = Some(String::new());
            self.settings_search_editing = true;
            self.status = t!("status.settings_search").into_owned();
            return;
        }
        match self.settings_focus {
            SettingsFocus::Category => self.on_settings_category_key(key),
            SettingsFocus::Detail => self.on_settings_detail_key(key),
        }
    }

    fn on_settings_category_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.settings_category_idx > 0 {
                    self.settings_category_idx -= 1;
                    self.select_first_setting_in_category();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.settings_category_idx + 1 < SETTING_CATEGORIES.len() {
                    self.settings_category_idx += 1;
                    self.select_first_setting_in_category();
                }
            }
            KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
                self.settings_focus = SettingsFocus::Detail;
                self.select_first_setting_in_category();
                self.show_setting_desc();
            }
            _ => {}
        }
    }

    fn on_settings_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left => self.edit_current(EditAction::Prev),
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_setting_in_category(-1);
                self.show_setting_desc();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_setting_in_category(1);
                self.show_setting_desc();
            }
            KeyCode::Char(' ') => self.edit_current(EditAction::Toggle),
            KeyCode::Enter if self.selected_setting_kind() == Some(KeyKind::Color) => {
                self.settings_edit_buf = self
                    .settings
                    .get(self.settings_idx)
                    .map(|row| row.value.clone());
                self.status = t!("status.settings_editing_color").into_owned();
            }
            KeyCode::Enter => self.edit_current(EditAction::Toggle),
            KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('=') => {
                self.edit_current(EditAction::Next)
            }
            KeyCode::Char('-') => self.edit_current(EditAction::Prev),
            KeyCode::Char('r') => self.reset_current_setting(),
            _ => {}
        }
    }

    fn selected_setting_kind(&self) -> Option<KeyKind> {
        self.settings.get(self.settings_idx).map(|row| row.kind)
    }

    fn select_first_setting_in_category(&mut self) {
        let indices = self.setting_indices_in_active_category();
        if let Some(first) = indices.first() {
            self.settings_idx = *first;
        }
    }

    fn settings_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.settings_search_editing = false;
                if self
                    .settings_search
                    .as_ref()
                    .is_some_and(|query| query.trim().is_empty())
                {
                    self.settings_search = None;
                }
                self.status = t!("status.settings_search_done").into_owned();
            }
            KeyCode::Esc => {
                self.settings_search = None;
                self.settings_search_editing = false;
                self.status = t!("hint.settings").into_owned();
            }
            KeyCode::Backspace => {
                if let Some(buf) = self.settings_search.as_mut() {
                    buf.pop();
                }
                self.select_first_setting_in_category();
            }
            KeyCode::Char(c) => {
                if let Some(buf) = self.settings_search.as_mut() {
                    buf.push(c);
                }
                self.select_first_setting_in_category();
            }
            _ => {}
        }
    }

    fn settings_editing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let Some(raw) = self.settings_edit_buf.take() else {
                    return;
                };
                let Some(row) = self.settings.get(self.settings_idx).cloned() else {
                    return;
                };
                self.commit_setting_value(&row, raw);
            }
            KeyCode::Esc => {
                self.settings_edit_buf = None;
                self.status = t!("status.settings_edit_cancelled").into_owned();
            }
            KeyCode::Backspace => {
                if let Some(buf) = self.settings_edit_buf.as_mut() {
                    buf.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(buf) = self.settings_edit_buf.as_mut() {
                    buf.push(c);
                }
            }
            _ => {}
        }
    }

    fn reset_current_setting(&mut self) {
        let Some(row) = self.settings.get(self.settings_idx).cloned() else {
            return;
        };
        match config::default_value(row.key) {
            Ok(raw) => self.commit_setting_value(&row, raw),
            Err(e) => self.status = t!("status.field_error", key = row.key, err = e).into_owned(),
        }
    }

    fn move_setting_in_category(&mut self, delta: isize) {
        let indices = self.setting_indices_in_active_category();
        if indices.is_empty() {
            return;
        }
        let current_pos = indices
            .iter()
            .position(|idx| *idx == self.settings_idx)
            .unwrap_or(0);
        let next_pos = if delta < 0 {
            current_pos.saturating_sub(1)
        } else {
            (current_pos + 1).min(indices.len() - 1)
        };
        self.settings_idx = indices[next_pos];
    }

    fn setting_indices_in_active_category(&self) -> Vec<usize> {
        let search = self
            .settings_search
            .as_ref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        self.settings
            .iter()
            .enumerate()
            .filter_map(|(idx, row)| {
                let in_category = self.settings_category_idx == 0
                    || setting_category_index(row.key) == self.settings_category_idx;
                let matches_search = search.as_ref().map_or(true, |needle| {
                    row.key.to_ascii_lowercase().contains(needle)
                        || row.value.to_ascii_lowercase().contains(needle)
                        || setting_desc(row.key).to_ascii_lowercase().contains(needle)
                });
                if in_category && matches_search {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect()
    }

    fn visible_setting_indices(&self, active_indices: &[usize], table_height: u16) -> Vec<usize> {
        if active_indices.is_empty() {
            return Vec::new();
        }
        let visible = settings_table_row_capacity(table_height).min(active_indices.len());
        let selected_pos = active_indices
            .iter()
            .position(|idx| *idx == self.settings_idx)
            .unwrap_or(0);
        let offset = selected_pos.saturating_add(1).saturating_sub(visible);
        active_indices[offset..offset + visible].to_vec()
    }

    /// Put the selected setting's description in the status bar.
    fn show_setting_desc(&mut self) {
        if let Some(row) = self.settings.get(self.settings_idx) {
            self.status = setting_desc(row.key);
        }
    }

    /// Apply an edit to the selected setting and persist it.
    fn edit_current(&mut self, action: EditAction) {
        let Some(row) = self.settings.get(self.settings_idx).cloned() else {
            return;
        };
        let Some(mut raw) = edit::next_raw(row.kind, &row.value, action) else {
            self.status = t!("status.cannot_edit", key = row.key).into_owned();
            return;
        };
        if is_selection_color_key(row.key) {
            let Some(valid) = self.next_valid_selection_color_raw(&row, action) else {
                self.status = t!("status.cannot_edit", key = row.key).into_owned();
                return;
            };
            raw = valid;
        }
        self.commit_setting_value(&row, raw);
    }

    fn next_valid_selection_color_raw(
        &self,
        row: &SettingRow,
        action: EditAction,
    ) -> Option<String> {
        let existing = std::fs::read_to_string(&self.config_path).ok();
        let mut current = row.value.clone();
        for _ in 0..16 {
            let candidate = edit::next_raw(row.kind, &current, action)?;
            if config::set_value(existing.as_deref(), row.key, &candidate).is_ok() {
                return Some(candidate);
            }
            current = candidate;
        }
        None
    }

    fn commit_setting_value(&mut self, row: &SettingRow, raw: String) {
        // Autostart is not just a config flag — it must register/remove the OS
        // logon entry. Delegate to `ai-handoff autostart on|off`, which writes
        // the config *and* applies the registry/scheduled-task change.
        if row.key == "autostart.enabled" {
            let on = raw == "true";
            match apply_autostart(on) {
                Ok(()) => {
                    self.settings[self.settings_idx].value = raw.clone();
                    self.status = if on {
                        t!("status.autostart_on")
                    } else {
                        t!("status.autostart_off")
                    }
                    .into_owned();
                }
                Err(e) => self.status = t!("status.autostart_failed", err = e).into_owned(),
            }
            return;
        }
        match edit::commit(&self.config_path, row.key, &raw) {
            Ok(_) => {
                // Language takes effect immediately: switch the global locale so
                // the next frame renders translated.
                if row.key == "language" {
                    rust_i18n::set_locale(&raw);
                }
                self.settings[self.settings_idx].value = raw.clone();
                if row.key.starts_with("theme.") {
                    self.refresh_theme_from_disk();
                }
                self.status = t!("status.saved", key = row.key, value = raw).into_owned();
            }
            Err(e) => self.status = t!("status.field_error", key = row.key, err = e).into_owned(),
        }
    }

    fn on_capsule_key(&mut self, key: KeyEvent) {
        match self.cap_focus {
            CapFocus::Tree => self.cap_tree_key(key),
            CapFocus::Detail => self.cap_detail_key(key),
            // Editing is intercepted in on_key before reaching here.
            CapFocus::Editing => self.cap_editing_key(key),
        }
    }

    /// Keys while the left tree (agent → project → capsule) has focus.
    fn cap_tree_key(&mut self, key: KeyEvent) {
        let rows = self.cap_rows();
        if rows.is_empty() {
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.cap_sel > 0 {
                    self.cap_sel -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cap_sel + 1 < rows.len() {
                    self.cap_sel += 1;
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ') => {
                match rows.get(self.cap_sel).map(|r| r.target) {
                    // A capsule: cross into the detail pane (the 7/10 side).
                    Some(CapTarget::Capsule(..)) => self.cap_enter_detail(),
                    Some(CapTarget::Agent(ai)) => toggle(&mut self.cap_expanded_agents, ai),
                    Some(CapTarget::Project(ai, pi)) => {
                        toggle(&mut self.cap_expanded_projects, (ai, pi))
                    }
                    None => {}
                }
            }
            KeyCode::Left => self.cap_collapse(&rows),
            _ => {}
        }
        // Clamp selection (an expand/collapse may have changed the row count).
        let n = self.cap_rows().len();
        if self.cap_sel >= n {
            self.cap_sel = n.saturating_sub(1);
        }
        self.cap_load_content();
    }

    /// ← collapses the agent/project; on a capsule it does nothing.
    fn cap_collapse(&mut self, rows: &[CapRow]) {
        match rows.get(self.cap_sel).map(|r| r.target) {
            Some(CapTarget::Agent(ai)) => {
                self.cap_expanded_agents.remove(&ai);
            }
            Some(CapTarget::Project(ai, pi)) => {
                self.cap_expanded_projects.remove(&(ai, pi));
            }
            _ => {}
        }
    }

    /// Cross from the tree into the detail pane for the selected capsule.
    fn cap_enter_detail(&mut self) {
        self.cap_focus = CapFocus::Detail;
        self.cap_field = 0;
        self.cap_confirm_delete = false;
        self.cap_load_content();
        self.status = t!("hint.capsule_detail").into_owned();
    }

    /// Keys while the right detail pane has focus (action bar + body).
    fn cap_detail_key(&mut self, key: KeyEvent) {
        // Any key other than a second 'd'/'y' cancels an armed delete.
        let confirming = self.cap_confirm_delete;
        if confirming && !matches!(key.code, KeyCode::Char('d') | KeyCode::Char('y')) {
            self.cap_confirm_delete = false;
            self.status = t!("status.delete_cancelled").into_owned();
        }
        match key.code {
            KeyCode::Left => self.cap_focus_tree(),
            KeyCode::Up | KeyCode::Char('k') => {
                if self.cap_field > 0 {
                    self.cap_field -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cap_field + 1 < CAP_FIELDS.len() {
                    self.cap_field += 1;
                }
            }
            KeyCode::Char('s') => self.cap_toggle_state(),
            KeyCode::Char('d') | KeyCode::Char('y') if confirming => self.cap_delete(),
            KeyCode::Char('d') => {
                self.cap_confirm_delete = true;
                self.status = t!("status.delete_confirm").into_owned();
            }
            KeyCode::Char('e') | KeyCode::Enter => self.cap_begin_edit(),
            _ => {}
        }
    }

    /// Return focus to the tree (the 3/10 side).
    fn cap_focus_tree(&mut self) {
        self.cap_focus = CapFocus::Tree;
        self.cap_confirm_delete = false;
        self.status = t!("hint.capsule_tree").into_owned();
    }

    /// Keys while editing the selected capsule field.
    fn cap_editing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.cap_commit_edit(),
            KeyCode::Esc => {
                self.cap_focus = CapFocus::Detail;
                self.status = t!("status.edit_cancelled").into_owned();
            }
            KeyCode::Backspace => {
                self.cap_edit_buf.pop();
            }
            KeyCode::Char(c) => self.cap_edit_buf.push(c),
            _ => {}
        }
    }

    /// The (agent, project, capsule) indices of the current selection, if it is
    /// a capsule.
    fn selected_capsule(&self) -> Option<(usize, usize, usize)> {
        match self.cap_rows().get(self.cap_sel).map(|r| r.target) {
            Some(CapTarget::Capsule(ai, pi, ci)) => Some((ai, pi, ci)),
            _ => None,
        }
    }

    /// Toggle the selected capsule's consumption state on disk and in the tree.
    fn cap_toggle_state(&mut self) {
        let Some((ai, pi, ci)) = self.selected_capsule() else {
            return;
        };
        let path = self.cap_tree[ai].projects[pi].capsules[ci].path.clone();
        match capsule_ops::toggle_state(Path::new(&path)) {
            Ok(new_state) => {
                self.cap_tree[ai].projects[pi].capsules[ci].state = new_state.clone();
                self.cap_detail = None; // force a re-read
                self.cap_load_content();
                let shown = state_label(&new_state);
                self.status = t!("status.state_changed", state = shown).into_owned();
            }
            Err(e) => self.status = t!("status.state_failed", err = e).into_owned(),
        }
    }

    /// Begin editing the selected field (loads its current text into the buffer).
    fn cap_begin_edit(&mut self) {
        let field = CAP_FIELDS[self.cap_field];
        let current = self
            .cap_detail
            .as_ref()
            .and_then(|d| d.parsed.as_ref())
            .map(|c| capsule_ops::field_text(c, field));
        match current {
            Some(text) => {
                self.cap_edit_buf = text;
                self.cap_focus = CapFocus::Editing;
                let name = field_label(field);
                self.status = if field.is_list() {
                    t!("status.editing_list", field = name)
                } else {
                    t!("status.editing", field = name)
                }
                .into_owned();
            }
            None => self.status = t!("status.cannot_edit_capsule").into_owned(),
        }
    }

    /// Save the edited field to disk and refresh the in-memory tree.
    fn cap_commit_edit(&mut self) {
        let Some((ai, pi, ci)) = self.selected_capsule() else {
            self.cap_focus = CapFocus::Detail;
            return;
        };
        let field = CAP_FIELDS[self.cap_field];
        let path = self.cap_tree[ai].projects[pi].capsules[ci].path.clone();
        let text = self.cap_edit_buf.clone();
        match capsule_ops::set_field(Path::new(&path), field, &text) {
            Ok(()) => {
                // Keep the tree's preview (the goal) in sync when it changes.
                if field == capsule_ops::CapField::Goal {
                    self.cap_tree[ai].projects[pi].capsules[ci].summary_preview = text;
                }
                self.cap_detail = None;
                self.cap_load_content();
                self.status = t!("status.field_saved", field = field_label(field)).into_owned();
            }
            Err(e) => self.status = t!("status.save_failed", err = e).into_owned(),
        }
        self.cap_focus = CapFocus::Detail;
    }

    /// Delete the selected capsule from disk and prune it from the tree.
    fn cap_delete(&mut self) {
        self.cap_confirm_delete = false;
        let Some((ai, pi, ci)) = self.selected_capsule() else {
            return;
        };
        let path = self.cap_tree[ai].projects[pi].capsules[ci].path.clone();
        if let Err(e) = capsule_ops::delete(Path::new(&path)) {
            self.status = t!("status.delete_failed", err = e).into_owned();
            return;
        }
        // Prune the capsule, then any now-empty project / agent.
        let proj = &mut self.cap_tree[ai].projects[pi];
        proj.capsules.remove(ci);
        self.cap_tree[ai].count = self.cap_tree[ai].count.saturating_sub(1);
        if self.cap_tree[ai].projects[pi].capsules.is_empty() {
            self.cap_tree[ai].projects.remove(pi);
        }
        if self.cap_tree[ai].projects.is_empty() {
            self.cap_tree.remove(ai);
        }
        // Selection/focus return to the (smaller) tree.
        let n = self.cap_rows().len();
        if self.cap_sel >= n {
            self.cap_sel = n.saturating_sub(1);
        }
        self.cap_focus = CapFocus::Tree;
        self.cap_detail = None;
        self.cap_load_content();
        self.status = t!("status.capsule_deleted").into_owned();
    }

    /// Read + parse the selected capsule into the detail cache (skipped when the
    /// selection is an agent/project, or unchanged from the last read).
    fn cap_load_content(&mut self) {
        let target = self.cap_rows().get(self.cap_sel).map(|r| r.target);
        if let Some(CapTarget::Capsule(ai, pi, ci)) = target {
            let path = self.cap_tree[ai].projects[pi].capsules[ci].path.clone();
            let already = self
                .cap_detail
                .as_ref()
                .map(|d| d.path == path)
                .unwrap_or(false);
            if !already {
                self.cap_field = 0;
                let res = ai_handoff_core::dashboard::read_capsule(Path::new(&path), 64 * 1024);
                let raw = match res.error {
                    Some(e) => format!("(could not read {path}: {e})"),
                    None => res.text,
                };
                let parsed = ai_handoff_core::capsule_codec::read_capsule(Path::new(&path)).ok();
                self.cap_detail = Some(CapDetail { path, parsed, raw });
            }
        } else {
            self.cap_detail = None;
        }
    }

    /// The flattened, currently-visible rows of the capsule tree.
    fn cap_rows(&self) -> Vec<CapRow> {
        let mut rows = Vec::new();
        for (ai, agent) in self.cap_tree.iter().enumerate() {
            let a_exp = self.cap_expanded_agents.contains(&ai);
            rows.push(CapRow {
                indent: 0,
                label: format!(
                    "{} {} ({})",
                    if a_exp { "▾" } else { "▸" },
                    agent.agent,
                    agent.count
                ),
                target: CapTarget::Agent(ai),
            });
            if !a_exp {
                continue;
            }
            for (pi, proj) in agent.projects.iter().enumerate() {
                let p_exp = self.cap_expanded_projects.contains(&(ai, pi));
                rows.push(CapRow {
                    indent: 2,
                    label: format!(
                        "{} {} ({})",
                        if p_exp { "▾" } else { "▸" },
                        project_label(&proj.project_id),
                        proj.capsules.len()
                    ),
                    target: CapTarget::Project(ai, pi),
                });
                if !p_exp {
                    continue;
                }
                for (ci, cap) in proj.capsules.iter().enumerate() {
                    rows.push(CapRow {
                        indent: 4,
                        label: format!("• {}", capsule_label(cap)),
                        target: CapTarget::Capsule(ai, pi, ci),
                    });
                }
            }
        }
        rows
    }

    // --- Account tab ---------------------------------------------------

    fn on_account_key(&mut self, key: KeyEvent) {
        match self.acc_focus {
            AccFocus::Tree => self.acc_tree_key(key),
            AccFocus::Detail => self.acc_detail_key(key),
        }
        // Whatever moved the selection, make sure the now-selected account's
        // usage is being fetched (cached after the first time).
        self.acc_ensure_usage(false);
    }

    /// Cache key for an account's fetched usage.
    fn usage_key(agent: Agent, label: &str) -> String {
        format!("{agent:?}:{label}")
    }

    /// Kick off a background usage fetch for the selected account. No-op if
    /// already cached unless `force`.
    fn acc_ensure_usage(&mut self, force: bool) {
        let Some((agent, i)) = self.acc_selected_slot() else {
            return;
        };
        let label = self.account.agent(agent).slots[i].meta.label.clone();
        self.acc_ensure_slot_usage(agent, label, force);
    }

    fn acc_ensure_slot_usage(&mut self, agent: Agent, label: String, force: bool) {
        let key = Self::usage_key(agent, &label);
        if !force && self.acc_usage.contains_key(&key) {
            return;
        }
        self.acc_usage.insert(key.clone(), UsageState::Loading);
        let tx = self.usage_tx.clone();
        std::thread::spawn(move || {
            let res = crate::account_api::fetch_slot_usage(agent, &label);
            let _ = tx.send((key, res));
        });
    }

    fn ensure_overview_limit_usage(&mut self) {
        let labels = [Agent::Claude, Agent::Codex]
            .into_iter()
            .filter_map(|agent| {
                self.account
                    .agent(agent)
                    .slots
                    .iter()
                    .find(|slot| slot.active)
                    .map(|slot| (agent, slot.meta.label.clone()))
            })
            .collect::<Vec<_>>();
        for (agent, label) in labels {
            self.acc_ensure_slot_usage(agent, label, false);
        }
    }

    /// The flattened, visible rows of the account tree (both agents, always
    /// expanded: header → saved accounts → "+ capture current").
    fn acc_rows(&self) -> Vec<AccRow> {
        let mut rows = Vec::new();
        for agent in [Agent::Codex, Agent::Claude] {
            let data = self.account.agent(agent);
            rows.push(AccRow {
                indent: 0,
                label: agent_name(agent).to_string(),
                target: AccTarget::Header(agent),
            });
            for (i, slot) in data.slots.iter().enumerate() {
                let display = slot.meta.email.as_deref().unwrap_or(&slot.meta.label);
                let label = if slot.active {
                    format!("• {}  [{}]", display, t!("account.active"))
                } else {
                    format!("• {}", display)
                };
                rows.push(AccRow {
                    indent: 2,
                    label,
                    target: AccTarget::Slot(agent, i),
                });
            }
            rows.push(AccRow {
                indent: 2,
                label: t!("account.add").into_owned(),
                target: AccTarget::Add(agent),
            });
        }
        rows
    }

    fn acc_target(&self) -> Option<AccTarget> {
        self.acc_rows().get(self.acc_sel).map(|r| r.target)
    }

    /// The agent owning the selected row (Codex when nothing is selected).
    fn acc_selected_agent(&self) -> Agent {
        match self.acc_target() {
            Some(AccTarget::Header(a) | AccTarget::Slot(a, _) | AccTarget::Add(a)) => a,
            None => Agent::Codex,
        }
    }

    /// The (agent, slot index) of the selection, if it is a saved account.
    fn acc_selected_slot(&self) -> Option<(Agent, usize)> {
        match self.acc_target() {
            Some(AccTarget::Slot(a, i)) => Some((a, i)),
            _ => None,
        }
    }

    /// Keys while the left account tree has focus.
    fn acc_tree_key(&mut self, key: KeyEvent) {
        let rows = self.acc_rows();
        if rows.is_empty() {
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.acc_sel > 0 {
                    self.acc_sel -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.acc_sel + 1 < rows.len() {
                    self.acc_sel += 1;
                }
            }
            KeyCode::Char('+') | KeyCode::Char('a') => {
                self.pending = Some(Pending::AddAccount(self.acc_selected_agent()));
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ') => {
                match rows.get(self.acc_sel).map(|r| r.target) {
                    Some(AccTarget::Add(agent)) => self.pending = Some(Pending::AddAccount(agent)),
                    Some(AccTarget::Slot(..)) | Some(AccTarget::Header(_)) => {
                        self.acc_focus = AccFocus::Detail;
                        self.acc_confirm_delete = false;
                        self.status = t!("hint.account_detail").into_owned();
                    }
                    None => {}
                }
            }
            _ => {}
        }
        let n = self.acc_rows().len();
        if self.acc_sel >= n {
            self.acc_sel = n.saturating_sub(1);
        }
    }

    /// Keys while the right detail pane has focus (switch / delete / refresh).
    fn acc_detail_key(&mut self, key: KeyEvent) {
        let confirming = self.acc_confirm_delete;
        if confirming && !matches!(key.code, KeyCode::Char('d') | KeyCode::Char('y')) {
            self.acc_confirm_delete = false;
            self.status = t!("status.account_delete_cancelled").into_owned();
        }
        match key.code {
            KeyCode::Left => {
                self.acc_focus = AccFocus::Tree;
                self.acc_confirm_delete = false;
                self.status = t!("hint.account_tree").into_owned();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.acc_sel > 0 {
                    self.acc_sel -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.acc_sel + 1 < self.acc_rows().len() {
                    self.acc_sel += 1;
                }
            }
            KeyCode::Char('s') => self.acc_switch(),
            KeyCode::Char('d') | KeyCode::Char('y') if confirming => self.acc_delete(),
            KeyCode::Char('d') => {
                if self.acc_selected_slot().is_some() {
                    self.acc_confirm_delete = true;
                    self.status = t!("status.account_delete_confirm").into_owned();
                }
            }
            KeyCode::Char('a') => {
                self.pending = Some(Pending::AddAccount(self.acc_selected_agent()))
            }
            KeyCode::Char('l') => {
                if let Some((agent, i)) = self.acc_selected_slot() {
                    let label = self.account.agent(agent).slots[i].meta.label.clone();
                    self.pending = Some(Pending::Launch(agent, label));
                }
            }
            KeyCode::Char('r') => self.acc_ensure_usage(true),
            _ => {}
        }
    }

    /// Make the selected saved account the live one (file swap).
    fn acc_switch(&mut self) {
        let Some((agent, i)) = self.acc_selected_slot() else {
            return;
        };
        let slot = &self.account.agent(agent).slots[i];
        let label = slot.meta.label.clone();
        // Managed (Business/Team/Enterprise) accounts: a forced workspace policy
        // may log the agent out on switch — caution the user.
        let plan = slot
            .meta
            .plan_hint
            .clone()
            .unwrap_or_default()
            .to_lowercase();
        let managed = ["business", "team", "enterprise"]
            .iter()
            .any(|k| plan.contains(k));
        // Warn (don't block) if a session is open — it may keep the old account.
        let running = account::agent_running(agent);
        match account::switch_slot(agent, &label) {
            Ok(()) => {
                self.reload_account();
                self.status = if managed {
                    t!("status.account_switched_managed", label = label)
                } else if running {
                    t!("status.account_switched_running", label = label)
                } else {
                    t!("status.account_switched", label = label)
                }
                .into_owned();
            }
            Err(e) => {
                self.status = t!("status.account_switch_failed", err = e.to_string()).into_owned()
            }
        }
    }

    /// Remove the selected saved account from the pool.
    fn acc_delete(&mut self) {
        self.acc_confirm_delete = false;
        let Some((agent, i)) = self.acc_selected_slot() else {
            return;
        };
        let label = self.account.agent(agent).slots[i].meta.label.clone();
        match account::delete_slot(agent, &label) {
            Ok(()) => {
                self.reload_account();
                self.status = t!("status.account_deleted").into_owned();
            }
            Err(e) => {
                self.status = t!("status.account_delete_failed", err = e.to_string()).into_owned()
            }
        }
    }

    /// Re-scan the live system after a pool change and clamp the selection. The
    /// usage cache is dropped (the set of accounts may have changed).
    fn reload_account(&mut self) {
        self.account = AccountData::load_live();
        self.acc_usage.clear();
        let n = self.acc_rows().len();
        if self.acc_sel >= n {
            self.acc_sel = n.saturating_sub(1);
        }
        self.acc_ensure_usage(false);
    }

    // --- drawing -------------------------------------------------------

    fn draw(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(3),
            ])
            .split(f.area());

        self.draw_tabs(f, chunks[0]);
        match self.tab {
            Tab::Overview => self.draw_overview(f, chunks[1]),
            Tab::Capsule => self.draw_capsule(f, chunks[1]),
            Tab::Usage => self.draw_usage(f, chunks[1]),
            Tab::Account => self.draw_account(f, chunks[1]),
            Tab::Integration => self.draw_integration(f, chunks[1]),
            Tab::Settings => self.draw_settings(f, chunks[1]),
        }
        self.draw_status(f, chunks[2]);
    }

    fn draw_tabs(&self, f: &mut Frame, area: Rect) {
        let titles = Tab::ALL
            .iter()
            .map(|tab| Line::from(t!(tab.title_key()).into_owned()));
        let tabs = Tabs::new(titles)
            .select(self.tab.index())
            .block(Block::default().borders(Borders::ALL).title("AI Handoff"))
            .highlight_style(self.selection_style().add_modifier(Modifier::BOLD));
        f.render_widget(tabs, area);
    }

    fn draw_overview(&self, f: &mut Frame, area: Rect) {
        if area.width >= 100 {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(7),
                    Constraint::Length(7),
                    Constraint::Min(5),
                ])
                .split(area);
            self.draw_health_strip(f, rows[0]);

            let top = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(rows[1]);
            self.draw_text_panel(
                f,
                top[0],
                t!("overview.actions").into_owned(),
                self.action_center_lines(),
            );
            self.draw_text_panel(
                f,
                top[1],
                t!("overview.current_project").into_owned(),
                self.current_project_lines(),
            );

            let middle = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(rows[2]);
            self.draw_text_panel(
                f,
                middle[0],
                t!("tab.capsule").into_owned(),
                self.capsule_summary_lines(),
            );
            self.draw_text_panel(
                f,
                middle[1],
                t!("overview.agent_limits").into_owned(),
                self.overview_limit_lines(),
            );
            self.draw_text_panel(
                f,
                rows[3],
                t!("overview.recent_activity").into_owned(),
                self.recent_activity_lines(),
            );
        } else {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(6),
                    Constraint::Length(5),
                    Constraint::Length(5),
                    Constraint::Length(5),
                    Constraint::Min(4),
                ])
                .split(area);
            self.draw_health_strip(f, rows[0]);
            self.draw_text_panel(
                f,
                rows[1],
                t!("overview.actions").into_owned(),
                self.action_center_lines(),
            );
            self.draw_text_panel(
                f,
                rows[2],
                t!("overview.current_project").into_owned(),
                self.current_project_lines(),
            );
            self.draw_text_panel(
                f,
                rows[3],
                t!("tab.capsule").into_owned(),
                self.capsule_summary_lines(),
            );
            self.draw_text_panel(
                f,
                rows[4],
                t!("overview.agent_limits").into_owned(),
                self.overview_limit_lines(),
            );
            self.draw_text_panel(
                f,
                rows[5],
                t!("overview.recent_activity").into_owned(),
                self.recent_activity_lines(),
            );
        }
    }

    fn draw_health_strip(&self, f: &mut Frame, area: Rect) {
        let mut spans = Vec::new();
        for check in &self.snapshot.checks {
            let (sym, color) = status_style(&check.status);
            spans.push(Span::styled(
                format!(" {} {} ", check.label, sym),
                Style::default().fg(color),
            ));
        }
        let para = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(t!("overview.health")),
        );
        f.render_widget(para, area);
    }

    fn draw_text_panel(
        &self,
        f: &mut Frame,
        area: Rect,
        title: impl Into<String>,
        lines: Vec<String>,
    ) {
        self.draw_text_panel_with_focus(f, area, title, lines, false);
    }

    fn draw_text_panel_with_focus(
        &self,
        f: &mut Frame,
        area: Rect,
        title: impl Into<String>,
        lines: Vec<String>,
        focused: bool,
    ) {
        let lines = lines.into_iter().map(Line::from).collect::<Vec<_>>();
        let para = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(self.focus_block(title, focused));
        f.render_widget(para, area);
    }

    fn draw_usage(&self, f: &mut Frame, area: Rect) {
        let total = self.usage.total.tokens.total();
        if total == 0 {
            let para = Paragraph::new(t!("usage.no_logs").into_owned())
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(t!("tab.usage")),
                );
            f.render_widget(para, area);
            return;
        }

        let cols = if area.width >= 100 {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(12), Constraint::Min(6)])
                .split(area)
        };
        let chart = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(7), Constraint::Length(7)])
            .split(cols[0]);
        let chart_focused = self.focus_content && self.usage_focus == UsageFocus::Chart;
        let details_focused = self.focus_content && self.usage_focus == UsageFocus::Details;
        self.draw_token_donut(f, chart[0], chart_focused);
        self.draw_token_legend(f, chart[1], chart_focused);

        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Min(5)])
            .split(cols[1]);
        let summary = self
            .usage_summary_lines()
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        f.render_widget(
            Paragraph::new(summary)
                .wrap(Wrap { trim: true })
                .block(self.focus_block(t!("usage.summary"), details_focused)),
            right[0],
        );

        let groups = match self.usage_mode.dimension() {
            Some(dim) => self.usage.breakdown(dim),
            None => self.usage.breakdown(Dimension::Project),
        };
        let rows = groups.iter().take(8).map(|g| {
            let label = match self.usage_mode {
                UsageViewMode::Summary | UsageViewMode::Project => project_label(&g.key),
                UsageViewMode::Source => source_label(&g.key),
                UsageViewMode::Day | UsageViewMode::Model => g.key.clone(),
            };
            Row::new([
                Cell::from(label),
                Cell::from(human_tokens(g.tokens.total())),
                Cell::from(format!("{:.2}", g.cost_usd)),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Percentage(60),
                Constraint::Length(12),
                Constraint::Length(10),
            ],
        )
        .header(
            Row::new([
                t!(self.usage_mode.label_key()).into_owned(),
                t!("table.tokens").into_owned(),
                t!("table.est_cost").into_owned(),
            ])
            .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(self.focus_block(
            format!(
                "{} — {}",
                t!("usage.breakdown"),
                t!(self.usage_mode.label_key())
            ),
            details_focused,
        ));
        f.render_widget(table, right[1]);
    }

    fn draw_integration(&self, f: &mut Frame, area: Rect) {
        if self.integration_page != IntegrationPage::Home {
            self.draw_integration_page(f, area);
            return;
        }
        let rows = if area.width >= 100 {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(9),
                    Constraint::Length(8),
                    Constraint::Min(5),
                ])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(10),
                    Constraint::Length(7),
                    Constraint::Min(5),
                ])
                .split(area)
        };

        let top = if area.width >= 100 {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(rows[0])
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(5), Constraint::Length(5)])
                .split(rows[0])
        };
        self.draw_integration_status_table(
            f,
            top[0],
            self.focus_content && self.integration_focus == IntegrationFocus::Status,
        );
        self.draw_text_panel_with_focus(
            f,
            top[1],
            t!("integration.repair_actions").into_owned(),
            self.repair_action_lines(),
            self.focus_content && self.integration_focus == IntegrationFocus::Repair,
        );
        self.draw_hooks_table(
            f,
            rows[1],
            self.focus_content && self.integration_focus == IntegrationFocus::Hooks,
        );
        self.draw_text_panel_with_focus(
            f,
            rows[2],
            t!("integration.recent_diagnostics").into_owned(),
            self.integration_status_lines(),
            self.focus_content && self.integration_focus == IntegrationFocus::Diagnostics,
        );
    }

    fn draw_integration_page(&self, f: &mut Frame, area: Rect) {
        match self.integration_page {
            IntegrationPage::Home => return,
            IntegrationPage::Detail => self.draw_text_panel_with_focus(
                f,
                area,
                t!("integration.detail_title").into_owned(),
                self.integration_detail_lines(),
                self.focus_content,
            ),
            IntegrationPage::DoctorRun => self.draw_text_panel_with_focus(
                f,
                area,
                t!("integration.doctor_title").into_owned(),
                self.integration_output.clone(),
                self.focus_content,
            ),
            IntegrationPage::Logs => self.draw_text_panel_with_focus(
                f,
                area,
                t!("integration.logs_title").into_owned(),
                self.integration_logs.clone(),
                self.focus_content,
            ),
            IntegrationPage::RepairCenter => self.draw_repair_center(f, area),
        }
    }

    fn draw_repair_center(&self, f: &mut Frame, area: Rect) {
        let cols = if area.width >= 100 {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(10), Constraint::Min(6)])
                .split(area)
        };
        let actions = self.recommended_repair_actions();
        let lines = actions
            .iter()
            .enumerate()
            .map(|(idx, kind)| {
                let mut text = format!(
                    "{} {}",
                    if kind.requires_confirm() { "!" } else { " " },
                    t!(kind.label_key())
                );
                if kind.is_manual() {
                    text.push_str(" (manual)");
                }
                let style = if idx == self.repair_sel {
                    self.selection_style()
                } else {
                    Style::default()
                };
                Line::from(Span::styled(text, style))
            })
            .collect::<Vec<_>>();
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .block(self.focus_block(
                    t!("integration.repair_center").into_owned(),
                    self.focus_content,
                )),
            cols[0],
        );

        let selected = self.selected_repair_action();
        let mut detail = vec![
            t!(selected.label_key()).into_owned(),
            t!(selected.detail_key()).into_owned(),
        ];
        if selected.requires_confirm() {
            detail.push(t!("integration.repair_requires_confirm").into_owned());
            if self.repair_confirm {
                detail.push(t!("integration.repair_confirm_armed").into_owned());
            }
        }
        if !self.integration_output.is_empty() {
            detail.push(String::new());
            detail.push(t!("integration.latest_run").into_owned());
            detail.extend(self.integration_output.iter().take(12).cloned());
        }
        self.draw_text_panel_with_focus(
            f,
            cols[1],
            t!("integration.repair_detail").into_owned(),
            detail,
            self.focus_content,
        );
    }

    fn draw_integration_status_table(&self, f: &mut Frame, area: Rect, focused: bool) {
        let rows = health_rows(&self.snapshot)
            .into_iter()
            .map(health_table_row);
        let table = Table::new(
            rows,
            [
                Constraint::Length(18),
                Constraint::Length(8),
                Constraint::Min(10),
            ],
        )
        .header(
            Row::new([
                t!("table.check").into_owned(),
                t!("table.status").into_owned(),
                t!("table.detail").into_owned(),
            ])
            .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(self.focus_block(t!("integration.status").into_owned(), focused));
        f.render_widget(table, area);
    }

    fn draw_hooks_table(&self, f: &mut Frame, area: Rect, focused: bool) {
        let (_, claude_color) = status_style(&self.snapshot.claude_settings.status);
        let (_, codex_color) = status_style(&self.snapshot.codex_hooks.status);
        let claude = format!(
            "{} {}",
            status_style(&self.snapshot.claude_settings.status).0,
            self.snapshot.claude_settings.message
        );
        let codex = format!(
            "{} {}",
            status_style(&self.snapshot.codex_hooks.status).0,
            self.snapshot.codex_hooks.message
        );
        let rows = ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop"]
            .into_iter()
            .map(|event| {
                Row::new([
                    Cell::from(event),
                    Cell::from(claude.clone()).style(Style::default().fg(claude_color)),
                    Cell::from(codex.clone()).style(Style::default().fg(codex_color)),
                ])
            });
        let table = Table::new(
            rows,
            [
                Constraint::Length(18),
                Constraint::Percentage(41),
                Constraint::Percentage(41),
            ],
        )
        .header(
            Row::new([
                t!("integration.event").into_owned(),
                "Claude".to_string(),
                "Codex".to_string(),
            ])
            .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(self.focus_block(t!("integration.hooks").into_owned(), focused));
        f.render_widget(table, area);
    }

    fn draw_capsule(&self, f: &mut Frame, area: Rect) {
        let rows = self.cap_rows();
        if rows.is_empty() {
            let para = Paragraph::new(t!("capsule.empty").into_owned())
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(t!("tab.capsule").into_owned()),
                );
            f.render_widget(para, area);
            return;
        }

        // Tree list on the left (3) — capsule detail on the right (7).
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(area);

        let tree_focused = self.focus_content && self.cap_focus == CapFocus::Tree;
        let detail_focused = self.focus_content && self.cap_focus != CapFocus::Tree;

        // --- left: the tree ---
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let text = format!("{}{}", " ".repeat(r.indent), r.label);
                if i == self.cap_sel {
                    let style = if tree_focused {
                        self.selection_style()
                    } else {
                        Style::default().add_modifier(Modifier::REVERSED)
                    };
                    Line::from(Span::styled(text, style))
                } else if let CapTarget::Agent(ai) = r.target {
                    // Brand-color the agent rows (Codex = purple, ClaudeCode = orange).
                    match self.agent_label_color(&self.cap_tree[ai].agent) {
                        Some(c) => Line::from(Span::styled(
                            text,
                            Style::default().fg(c).add_modifier(Modifier::BOLD),
                        )),
                        None => Line::from(text),
                    }
                } else {
                    Line::from(text)
                }
            })
            .collect();
        let list =
            Paragraph::new(lines).block(self.focus_block(t!("capsule.list_title"), tree_focused));
        f.render_widget(list, cols[0]);

        // --- right: action bar (top) over the body (bottom) ---
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(cols[1]);
        self.draw_capsule_actions(f, right[0], detail_focused);
        self.draw_capsule_body(f, right[1], detail_focused);
    }

    /// The action bar: state-toggle / delete / edit, with their hotkeys shown.
    fn draw_capsule_actions(&self, f: &mut Frame, area: Rect, focused: bool) {
        let state = self
            .cap_detail
            .as_ref()
            .and_then(|d| d.parsed.as_ref())
            .map(|c| state_label(c.consumption.state.as_str()))
            .unwrap_or_else(|| "—".to_string());
        let mut spans = vec![
            Span::raw(" "),
            Span::styled(
                format!(" {} ", t!("capsule.state_label", state = state)),
                Style::default().fg(Color::Black).bg(Color::Gray),
            ),
            Span::raw("  "),
            Span::styled(
                format!(" {} ", t!("capsule.btn_toggle")),
                self.action_style(focused),
            ),
            Span::raw("  "),
        ];
        if self.cap_confirm_delete {
            spans.push(Span::styled(
                format!(" {} ", t!("capsule.btn_confirm_delete")),
                Style::default().fg(Color::White).bg(Color::Red),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {} ", t!("capsule.btn_delete")),
                self.action_style(focused),
            ));
        }
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(" {} ", t!("capsule.btn_edit")),
            self.action_style(focused),
        ));
        let bar = Paragraph::new(Line::from(spans)).block(self.focus_block(
            t!("capsule.actions"),
            focused && self.cap_focus == CapFocus::Detail,
        ));
        f.render_widget(bar, area);
    }

    /// The capsule body: the editable fields (selectable) + read-only context,
    /// or the field editor when editing.
    fn draw_capsule_body(&self, f: &mut Frame, area: Rect, focused: bool) {
        let detail_active = focused && self.cap_focus == CapFocus::Detail;

        if self.cap_focus == CapFocus::Editing {
            let field = CAP_FIELDS[self.cap_field];
            let hint = if field.is_list() {
                t!("capsule.edit_hint_list")
            } else {
                t!("capsule.edit_hint")
            };
            let banner = t!(
                "capsule.edit_banner",
                field = field_label(field),
                hint = hint
            );
            let lines = vec![
                Line::from(banner.into_owned()).italic(),
                Line::from(""),
                Line::from(vec![
                    Span::raw(self.cap_edit_buf.clone()),
                    Span::styled("▏", Style::default().fg(Color::Cyan)),
                ]),
            ];
            let editor = Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(self.focus_block(t!("capsule.edit_title"), true));
            f.render_widget(editor, area);
            return;
        }

        let (title, lines) = match self.cap_detail.as_ref() {
            Some(detail) => (
                t!("capsule.detail_title", path = detail.path).into_owned(),
                self.capsule_body_lines(detail),
            ),
            None => (
                t!("capsule.detail_label").into_owned(),
                vec![Line::from(t!("capsule.detail_placeholder").into_owned())],
            ),
        };
        let body = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(self.focus_block(title, detail_active));
        f.render_widget(body, area);
    }

    /// Lines for the capsule body: editable fields (the selected one highlighted
    /// when the detail pane is active) followed by read-only context.
    fn capsule_body_lines(&self, detail: &CapDetail) -> Vec<Line<'static>> {
        let Some(c) = detail.parsed.as_ref() else {
            return detail
                .raw
                .lines()
                .map(|l| Line::from(l.to_string()))
                .collect();
        };
        let detail_active = self.focus_content && self.cap_focus == CapFocus::Detail;
        let mut lines = vec![Line::from(Span::styled(
            t!("capsule.editable_header").into_owned(),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        for (i, field) in CAP_FIELDS.iter().enumerate() {
            let val = capsule_ops::field_text(c, *field);
            let shown = if val.is_empty() {
                t!("capsule.empty_value").into_owned()
            } else {
                val
            };
            let text = format!("  {:<14} {shown}", format!("{}:", field_label(*field)));
            let style = if i == self.cap_field && detail_active {
                self.selection_style()
            } else if i == self.cap_field {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(text, style)));
        }

        let state = state_label(c.consumption.state.as_str());
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            t!("capsule.readonly_header").into_owned(),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(format!(
            "  {}: {:?} → {:?}",
            t!("capsule.field_flow"),
            c.source_agent,
            c.target_agent
        )));
        lines.push(Line::from(format!(
            "  {}: {state}",
            t!("capsule.field_state")
        )));
        lines.push(Line::from(format!(
            "  {}: {}",
            t!("capsule.field_created"),
            c.created_at
        )));
        lines.push(Line::from(format!(
            "  {}: {}",
            t!("capsule.field_id"),
            c.capsule_id
        )));
        if !c.files.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {}", t!("capsule.field_files")),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for fch in &c.files {
                let status = fch.status.clone().unwrap_or_default();
                lines.push(Line::from(format!("    {status} {}", fch.path)));
            }
        }
        lines
    }

    /// Claude/Codex token totals from the per-source aggregation.
    fn source_split(&self) -> (u64, u64) {
        let mut claude = 0;
        let mut codex = 0;
        for g in &self.usage.by_source {
            match g.key.as_str() {
                "claude" => claude = g.tokens.total(),
                "codex" => codex = g.tokens.total(),
                _ => {}
            }
        }
        (claude, codex)
    }

    fn usage_summary_lines(&self) -> Vec<String> {
        let (claude, codex) = self.source_split();
        let total = self.usage.total.tokens.total();
        let mut lines = vec![
            t!("usage.total_tokens", tokens = human_tokens(total)).into_owned(),
            t!(
                "usage.estimate_line",
                cost = format!("{:.2}", self.usage.total.cost_usd)
            )
            .into_owned(),
            t!(
                "usage.source_tokens",
                source = "Claude",
                tokens = human_tokens(claude)
            )
            .into_owned(),
            t!(
                "usage.source_tokens",
                source = "Codex",
                tokens = human_tokens(codex)
            )
            .into_owned(),
        ];
        if self.usage.total.unpriced_tokens > 0 {
            lines.push(
                t!(
                    "usage.unpriced_tokens",
                    tokens = human_tokens(self.usage.total.unpriced_tokens)
                )
                .into_owned(),
            );
        }
        lines
    }

    fn action_center_lines(&self) -> Vec<String> {
        let mut rows: Vec<(u8, String)> = self
            .snapshot
            .checks
            .iter()
            .filter_map(|check| {
                let priority = match check.status {
                    CheckStatus::Error | CheckStatus::Missing => 0,
                    CheckStatus::Warning => 1,
                    CheckStatus::Unknown => 2,
                    CheckStatus::Ok => return None,
                };
                let (sym, _) = status_style(&check.status);
                Some((
                    priority,
                    format!("{sym} {}: {}", check.label, check.message),
                ))
            })
            .collect();
        if self.snapshot.capsules.pending_count > 0 {
            rows.push((
                3,
                format!(
                    "info {}",
                    t!(
                        "overview.pending_capsules",
                        count = self.snapshot.capsules.pending_count
                    )
                ),
            ));
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        let mut lines = rows.into_iter().map(|(_, line)| line).collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push(format!("ok {}", t!("overview.all_integrations_normal")));
            lines.push(format!("ok {}", t!("overview.no_pending_capsule")));
            lines.push(format!("ok {}", t!("overview.automatic_handoff_standby")));
        }
        lines.truncate(4);
        lines
    }

    fn current_project_lines(&self) -> Vec<String> {
        let repo = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "unknown".to_string());
        let mut lines = vec![
            t!("overview.repo_line", repo = repo).into_owned(),
            t!(
                "overview.pending_line",
                count = self.snapshot.capsules.pending_count
            )
            .into_owned(),
        ];
        if let Some(cap) = self.snapshot.capsules.items.first() {
            lines.push(
                t!(
                    "overview.last_flow_line",
                    source = cap.source_agent.clone(),
                    target = cap.target_agent.clone()
                )
                .into_owned(),
            );
            lines.push(
                t!(
                    "overview.last_capsule_line",
                    when = cap.created_at.get(..10).unwrap_or(&cap.created_at)
                )
                .into_owned(),
            );
        } else {
            lines.push(t!("overview.last_capsule_none").into_owned());
        }
        lines
    }

    fn capsule_summary_lines(&self) -> Vec<String> {
        let total = self.snapshot.capsules.items.len();
        let failed = self
            .snapshot
            .capsules
            .items
            .iter()
            .filter(|cap| cap.state == "failed")
            .count();
        let mut lines = vec![
            t!(
                "overview.pending_count",
                count = self.snapshot.capsules.pending_count
            )
            .into_owned(),
            t!("overview.total_count", count = total).into_owned(),
            t!("overview.failed_count", count = failed).into_owned(),
        ];
        for cap in self.snapshot.capsules.items.iter().take(3) {
            lines.push(format!(
                "{} {} -> {} {}",
                project_label(&cap.project_id),
                cap.source_agent,
                cap.target_agent,
                cap.created_at.get(..10).unwrap_or(&cap.created_at)
            ));
        }
        lines
    }

    fn overview_limit_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for agent in [Agent::Claude, Agent::Codex] {
            let limits = self.overview_agent_limits(agent);
            lines.push(overview_limit_line(
                &format!("{} 5h", agent_name(agent)),
                limits.five_hour.as_ref(),
                &limits.note,
            ));
            lines.push(overview_limit_line(
                &format!("{} week", agent_name(agent)),
                limits.weekly.as_ref(),
                &limits.note,
            ));
        }
        lines
    }

    fn overview_agent_limits(&self, agent: Agent) -> OverviewAgentLimits {
        let data = self.account.agent(agent);
        let active = data.slots.iter().find(|slot| slot.active);
        let mut limits = OverviewAgentLimits::default();

        if let Some(slot) = active {
            match self
                .acc_usage
                .get(&Self::usage_key(agent, &slot.meta.label))
            {
                Some(UsageState::Loaded(usage)) => {
                    limits.five_hour = usage.five_hour.clone();
                    limits.weekly = usage.weekly.clone();
                }
                Some(UsageState::Loading) => {
                    limits.note = Some(t!("overview.limit_loading").into_owned());
                }
                Some(UsageState::Error(err)) => {
                    limits.note = Some(t!("overview.limit_error", err = err.clone()).into_owned());
                }
                None => {
                    limits.note = Some(t!("overview.limit_loading").into_owned());
                }
            }
        } else {
            limits.note = Some(t!("overview.limit_no_account").into_owned());
        }

        if agent == Agent::Claude {
            if let Some(status) = data.status.as_ref() {
                if limits.five_hour.is_none() {
                    limits.five_hour = status.five_hour.clone();
                }
                if limits.weekly.is_none() {
                    limits.weekly = status.weekly.clone();
                }
            }
        }

        limits
    }

    fn recent_activity_lines(&self) -> Vec<String> {
        if self.snapshot.capsules.items.is_empty() {
            return vec![t!("overview.no_recent_activity").into_owned()];
        }
        self.snapshot
            .capsules
            .items
            .iter()
            .take(5)
            .map(|cap| {
                format!(
                    "{} {} {}",
                    cap.created_at.get(..16).unwrap_or(&cap.created_at),
                    cap.source_agent,
                    cap.state
                )
            })
            .collect()
    }

    fn integration_status_lines(&self) -> Vec<String> {
        self.snapshot
            .checks
            .iter()
            .map(|check| {
                let (sym, _) = status_style(&check.status);
                format!("{sym} {}: {}", check.label, check.message)
            })
            .collect()
    }

    fn integration_detail_lines(&self) -> Vec<String> {
        match self.integration_focus {
            IntegrationFocus::Status => {
                let mut lines = vec![t!("integration.detail_status").into_owned()];
                for check in &self.snapshot.checks {
                    let (sym, _) = status_style(&check.status);
                    lines.push(format!("{sym} {}: {}", check.label, check.message));
                    if let Some(path) = &check.path {
                        lines.push(format!("  {path}"));
                    }
                }
                lines
            }
            IntegrationFocus::Repair => {
                let mut lines = vec![t!("integration.detail_repair").into_owned()];
                lines.extend(self.repair_action_lines());
                lines
            }
            IntegrationFocus::Hooks => vec![
                t!("integration.detail_hooks").into_owned(),
                format!("Claude: {}", self.snapshot.claude_settings.message),
                format!("Codex: {}", self.snapshot.codex_hooks.message),
                format!("Codex hooks: {}", self.snapshot.paths.codex_hooks),
                format!("Codex config: {}", self.snapshot.paths.codex_config),
                format!("Claude settings: {}", self.snapshot.paths.claude_settings),
            ],
            IntegrationFocus::Diagnostics => {
                let mut lines = vec![t!("integration.detail_diagnostics").into_owned()];
                lines.extend(self.integration_status_lines());
                if self.snapshot.duplicates.is_empty() {
                    lines.push(t!("integration.no_duplicates").into_owned());
                } else {
                    lines.push(t!("integration.duplicates").into_owned());
                    for dup in &self.snapshot.duplicates {
                        lines.push(format!("warn {}: {}", dup.label, dup.message));
                    }
                }
                lines
            }
        }
    }

    fn repair_action_lines(&self) -> Vec<String> {
        self.recommended_repair_actions()
            .into_iter()
            .map(|kind| t!(kind.label_key()).into_owned())
            .collect()
    }

    fn recommended_repair_actions(&self) -> Vec<RepairActionKind> {
        let mut actions = Vec::new();
        let add = |actions: &mut Vec<RepairActionKind>, kind| {
            if !actions.contains(&kind) {
                actions.push(kind);
            }
        };
        for check in &self.snapshot.checks {
            if matches!(
                check.status,
                CheckStatus::Error | CheckStatus::Missing | CheckStatus::Warning
            ) {
                match check.id.as_str() {
                    "codex-hooks" | "claude-settings" | "codex-config" | "ipc" | "store" => {
                        add(&mut actions, RepairActionKind::InstallPlugin)
                    }
                    "daemon" => add(&mut actions, RepairActionKind::StartDaemon),
                    "autostart" if self.snapshot.install_state.autostart != "missing" => {
                        add(&mut actions, RepairActionKind::AutostartOn)
                    }
                    _ => {}
                }
            }
        }
        if !self.snapshot.duplicates.is_empty() {
            add(&mut actions, RepairActionKind::ManualLegacyCleanup);
        }
        if self
            .snapshot
            .codex_config
            .message
            .to_ascii_lowercase()
            .contains("trust")
        {
            add(&mut actions, RepairActionKind::ManualCodexTrust);
        }
        if actions.is_empty() {
            actions.push(RepairActionKind::RunDoctor);
        } else {
            add(&mut actions, RepairActionKind::RunDoctor);
        }
        actions
    }

    fn selected_repair_action(&self) -> RepairActionKind {
        let actions = self.recommended_repair_actions();
        actions
            .get(self.repair_sel.min(actions.len().saturating_sub(1)))
            .copied()
            .unwrap_or(RepairActionKind::RunDoctor)
    }

    fn activate_repair_selection(&mut self) {
        let kind = self.selected_repair_action();
        if kind.is_manual() {
            self.integration_output = vec![
                t!(kind.label_key()).into_owned(),
                t!(kind.detail_key()).into_owned(),
            ];
            self.repair_confirm = false;
            self.status = t!("status.repair_manual").into_owned();
            return;
        }
        if kind.requires_confirm() && !self.repair_confirm {
            self.repair_confirm = true;
            self.status = t!(
                "status.repair_confirm",
                action = t!(kind.label_key()).into_owned()
            )
            .into_owned();
            return;
        }
        self.repair_confirm = false;
        match kind {
            RepairActionKind::RunDoctor => self.run_integration_doctor(),
            RepairActionKind::InstallPlugin => {
                self.run_repair_command(kind, &["install", "--yes"], false)
            }
            RepairActionKind::StartDaemon => {
                self.run_repair_command(kind, &["daemon", "run"], true)
            }
            RepairActionKind::AutostartOn => {
                self.run_repair_command(kind, &["autostart", "on"], false)
            }
            RepairActionKind::ManualLegacyCleanup | RepairActionKind::ManualCodexTrust => {}
        }
    }

    fn run_repair_command(&mut self, kind: RepairActionKind, args: &[&str], spawn: bool) {
        let started = Instant::now();
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                self.integration_output = vec![format!("current_exe failed: {error}")];
                self.status = t!("status.repair_failed", err = error.to_string()).into_owned();
                return;
            }
        };
        let command_line = format!("{} {}", exe.to_string_lossy(), args.join(" "));
        let result = if spawn {
            Command::new(&exe)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|child| (0, format!("spawned pid {}", child.id())))
                .map_err(|error| error.to_string())
        } else {
            Command::new(&exe)
                .args(args)
                .output()
                .map(|output| {
                    let code = output.status.code().unwrap_or(-1);
                    let mut text = String::new();
                    text.push_str(&String::from_utf8_lossy(&output.stdout));
                    text.push_str(&String::from_utf8_lossy(&output.stderr));
                    (code, text)
                })
                .map_err(|error| error.to_string())
        };

        match result {
            Ok((code, text)) => {
                let elapsed = started.elapsed().as_millis();
                self.integration_output = vec![
                    format!("$ {command_line}"),
                    format!(
                        "{}: {}",
                        t!(kind.label_key()),
                        t!("integration.exit_code", code = code)
                    ),
                    format!("{}: {elapsed}ms", t!("integration.elapsed")),
                ];
                self.integration_output.extend(nonempty_lines(&text, 20));
                self.snapshot = ai_handoff_core::dashboard::dashboard_snapshot();
                self.integration_logs = self.integration_log_lines();
                self.status = if code == 0 {
                    t!("status.repair_done").into_owned()
                } else {
                    t!("status.repair_failed", err = format!("exit {code}")).into_owned()
                };
            }
            Err(error) => {
                self.integration_output = vec![format!("$ {command_line}"), error.clone()];
                self.status = t!("status.repair_failed", err = error).into_owned();
            }
        }
    }

    fn run_integration_doctor(&mut self) {
        let started = Instant::now();
        self.snapshot = ai_handoff_core::dashboard::dashboard_snapshot();
        let account = AccountData::load_live();
        let elapsed = started.elapsed().as_millis();
        let mut ok = 0;
        let mut warn = 0;
        let mut fail = 0;
        for check in &self.snapshot.checks {
            match check.status {
                CheckStatus::Ok => ok += 1,
                CheckStatus::Warning | CheckStatus::Unknown => warn += 1,
                CheckStatus::Error | CheckStatus::Missing => fail += 1,
            }
        }
        self.integration_output = vec![
            t!("integration.doctor_completed", ms = elapsed).into_owned(),
            t!(
                "integration.doctor_counts",
                ok = ok,
                warn = warn,
                fail = fail
            )
            .into_owned(),
            t!(
                "integration.account_summary",
                codex = account.codex.slots.len(),
                claude = account.claude.slots.len()
            )
            .into_owned(),
        ];
        self.integration_output
            .extend(self.integration_status_lines());
        self.integration_output
            .push(t!("integration.recommended_repairs").into_owned());
        self.integration_output.extend(
            self.recommended_repair_actions()
                .into_iter()
                .map(|kind| format!("- {}", t!(kind.label_key()))),
        );
        self.integration_page = IntegrationPage::DoctorRun;
        self.status = t!("status.doctor_done", ms = elapsed).into_owned();
    }

    fn open_integration_logs(&mut self) {
        self.integration_logs = self.integration_log_lines();
        self.integration_page = IntegrationPage::Logs;
        self.status = t!("hint.integration_logs").into_owned();
    }

    fn integration_log_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.integration_output.is_empty() {
            lines.push(t!("integration.latest_run").into_owned());
            lines.extend(self.integration_output.iter().take(30).cloned());
            lines.push(String::new());
        }
        for log in ai_handoff_core::dashboard::read_logs(32 * 1024) {
            lines.push(format!("== {} ==", log.name));
            if let Some(error) = log.result.error {
                lines.push(format!("{}: {error}", t!("integration.log_error")));
            } else if log.result.text.trim().is_empty() {
                lines.push(t!("integration.log_empty").into_owned());
            } else {
                lines.extend(nonempty_lines(&log.result.text, 40));
                if log.result.truncated {
                    lines.push(t!("integration.log_truncated").into_owned());
                }
            }
            lines.push(String::new());
        }
        if lines.is_empty() {
            lines.push(t!("integration.log_empty").into_owned());
        }
        lines
    }

    fn active_setting_category(&self) -> &'static SettingCategory {
        SETTING_CATEGORIES
            .get(self.settings_category_idx)
            .unwrap_or(&SETTING_CATEGORIES[0])
    }

    fn draw_token_donut(&self, f: &mut Frame, area: Rect, focused: bool) {
        let (claude, codex) = self.source_split();
        let total = claude + codex;
        let block = self.focus_block(t!("overview.token_split").into_owned(), focused);
        if total == 0 {
            f.render_widget(
                Paragraph::new(t!("overview.no_usage").into_owned()).block(block),
                area,
            );
            return;
        }
        let claude_frac = claude as f64 / total as f64;
        let claude_color = self.theme.claude;
        let codex_color = self.theme.codex;
        let canvas = Canvas::default()
            .block(block)
            .marker(Marker::Braille)
            .x_bounds([-1.6, 1.6])
            .y_bounds([-1.0, 1.0])
            .paint(move |ctx| {
                // A ring (annulus): walk the angle once, color each arc by which
                // source it belongs to, then plot a band of radii at that angle.
                let mut claude_pts: Vec<(f64, f64)> = Vec::new();
                let mut codex_pts: Vec<(f64, f64)> = Vec::new();
                let steps = 240;
                for i in 0..steps {
                    let frac = i as f64 / steps as f64;
                    // start at the top, go clockwise
                    let theta = std::f64::consts::FRAC_PI_2 - frac * std::f64::consts::TAU;
                    let bucket = if frac < claude_frac {
                        &mut claude_pts
                    } else {
                        &mut codex_pts
                    };
                    let mut r = 0.55;
                    while r <= 0.92 {
                        bucket.push((r * theta.cos() * 1.55, r * theta.sin()));
                        r += 0.04;
                    }
                }
                ctx.draw(&Points {
                    coords: &claude_pts,
                    color: claude_color,
                });
                ctx.draw(&Points {
                    coords: &codex_pts,
                    color: codex_color,
                });
            });
        f.render_widget(canvas, area);
    }

    fn draw_token_legend(&self, f: &mut Frame, area: Rect, focused: bool) {
        let (claude, codex) = self.source_split();
        let total = claude + codex;
        let pct = |n: u64| {
            if total > 0 {
                n as f64 / total as f64 * 100.0
            } else {
                0.0
            }
        };
        let total_line = t!(
            "overview.total",
            tokens = human_tokens(total),
            cost = format!("{:.2}", self.usage.total.cost_usd)
        );
        let lines = vec![
            Line::from(total_line.into_owned()),
            Line::from(vec![
                Span::styled("● claude  ", Style::default().fg(self.theme.claude)),
                Span::raw(format!(
                    "{:>7}  {:>4.0}%",
                    human_tokens(claude),
                    pct(claude)
                )),
            ]),
            Line::from(vec![
                Span::styled("● codex   ", Style::default().fg(self.theme.codex)),
                Span::raw(format!("{:>7}  {:>4.0}%", human_tokens(codex), pct(codex))),
            ]),
            Line::from(t!("overview.estimate").into_owned()).italic(),
        ];
        f.render_widget(
            Paragraph::new(lines).block(self.focus_block("", focused)),
            area,
        );
    }

    /// The Account tab: a Codex/Claude account tree (3/10) over a detail pane
    /// (7/10) with the selected agent's plan, limits, and reset credits.
    fn draw_account(&self, f: &mut Frame, area: Rect) {
        let rows = self.acc_rows();
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(area);

        let tree_focused = self.focus_content && self.acc_focus == AccFocus::Tree;
        let detail_focused = self.focus_content && self.acc_focus == AccFocus::Detail;

        // --- left: the account tree (agent headers carry the brand color) ---
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let text = format!("{}{}", " ".repeat(r.indent), r.label);
                if i == self.acc_sel {
                    let style = if tree_focused {
                        self.selection_style()
                    } else {
                        Style::default().add_modifier(Modifier::REVERSED)
                    };
                    Line::from(Span::styled(text, style))
                } else if let AccTarget::Header(agent) = r.target {
                    Line::from(Span::styled(
                        text,
                        Style::default()
                            .fg(self.agent_color(agent))
                            .add_modifier(Modifier::BOLD),
                    ))
                } else {
                    Line::from(text)
                }
            })
            .collect();
        let list =
            Paragraph::new(lines).block(self.focus_block(t!("account.list_title"), tree_focused));
        f.render_widget(list, cols[0]);

        // --- right: action bar (top) over the status pane (bottom) ---
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(cols[1]);
        self.draw_account_actions(f, right[0], detail_focused);
        self.draw_account_status(f, right[1], detail_focused);
    }

    /// The action bar: switch / delete for the selected saved account.
    fn draw_account_actions(&self, f: &mut Frame, area: Rect, focused: bool) {
        let has_slot = self.acc_selected_slot().is_some();
        let mut spans = vec![
            Span::raw(" "),
            Span::styled(
                format!(" {} ", t!("account.btn_switch")),
                self.action_style(focused && has_slot),
            ),
            Span::raw("  "),
            Span::styled(
                format!(" {} ", t!("account.btn_launch")),
                self.action_style(focused && has_slot),
            ),
            Span::raw("  "),
        ];
        if self.acc_confirm_delete {
            spans.push(Span::styled(
                format!(" {} ", t!("account.btn_confirm_delete")),
                Style::default().fg(Color::White).bg(Color::Red),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {} ", t!("account.btn_delete")),
                self.action_style(focused && has_slot),
            ));
        }
        let bar = Paragraph::new(Line::from(spans))
            .block(self.focus_block(t!("account.actions"), focused));
        f.render_widget(bar, area);
    }

    /// The status pane reflects the **selected** row: a saved account's details
    /// (live limits only when it is the active one), or an agent summary on the
    /// header / add rows.
    fn draw_account_status(&self, f: &mut Frame, area: Rect, focused: bool) {
        match self.acc_target() {
            Some(AccTarget::Slot(agent, i)) => self.draw_slot_detail(f, area, focused, agent, i),
            _ => self.draw_agent_summary(f, area, focused, self.acc_selected_agent()),
        }
    }

    /// Details for one saved account. Live plan/limits/credits show only when the
    /// slot is the active account (the live data belongs to whoever is signed in).
    fn draw_slot_detail(&self, f: &mut Frame, area: Rect, focused: bool, agent: Agent, i: usize) {
        let data = self.account.agent(agent);
        let Some(slot) = data.slots.get(i) else {
            return self.draw_agent_summary(f, area, focused, agent);
        };
        let block = self.focus_block(agent_name(agent).to_string(), focused);
        let inner = block.inner(area);
        f.render_widget(block, area);
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(2),
            ])
            .split(inner);

        // Resolve this account's own usage. Codex uses ChatGPT backend usage;
        // Claude uses the saved slot's OAuth usage endpoint. If Claude has no
        // fetched slot data yet, fall back to the active statusline sample.
        let mut plan = slot.meta.plan_hint.clone();
        let mut five: Option<RateWindow> = None;
        let mut weekly: Option<RateWindow> = None;
        let mut credits: Option<i64> = None;
        let mut credit_details: Vec<ResetCredit> = Vec::new();
        let mut note: Option<Line> = None;
        match agent {
            Agent::Codex => match self
                .acc_usage
                .get(&Self::usage_key(agent, &slot.meta.label))
            {
                Some(UsageState::Loaded(u)) => {
                    if u.plan.is_some() {
                        plan = u.plan.clone();
                    }
                    five = u.five_hour.clone();
                    weekly = u.weekly.clone();
                    credits = u.reset_credits;
                    credit_details = u.reset_credit_details.clone();
                }
                Some(UsageState::Loading) => {
                    note = Some(
                        Line::from(t!("account.usage_loading").into_owned()).fg(Color::DarkGray),
                    )
                }
                Some(UsageState::Error(e)) => {
                    note = Some(
                        Line::from(t!("account.usage_error", err = e.clone()).into_owned())
                            .fg(Color::Red),
                    )
                }
                None => {
                    note = Some(
                        Line::from(t!("account.usage_press_r").into_owned()).fg(Color::DarkGray),
                    )
                }
            },
            Agent::Claude => match self
                .acc_usage
                .get(&Self::usage_key(agent, &slot.meta.label))
            {
                Some(UsageState::Loaded(u)) => {
                    if u.plan.is_some() {
                        plan = u.plan.clone();
                    }
                    five = u.five_hour.clone();
                    weekly = u.weekly.clone();
                }
                Some(UsageState::Loading) => {
                    note = Some(
                        Line::from(t!("account.usage_loading").into_owned()).fg(Color::DarkGray),
                    )
                }
                Some(UsageState::Error(e)) => {
                    note = Some(
                        Line::from(t!("account.usage_error", err = e.clone()).into_owned())
                            .fg(Color::Red),
                    )
                }
                None => {
                    if slot.active {
                        if let Some(s) = data.status.as_ref() {
                            five = s.five_hour.clone();
                            weekly = s.weekly.clone();
                        }
                    } else {
                        note = Some(
                            Line::from(t!("account.usage_press_r").into_owned())
                                .fg(Color::DarkGray),
                        );
                    }
                }
            },
        }

        // Header: account email (+ active mark) + plan.
        let email = slot.meta.email.as_deref().unwrap_or(&slot.meta.label);
        let mut account_line = t!("account.account_line", email = email).into_owned();
        if slot.active {
            account_line.push_str(&format!("  [{}]", t!("account.active")));
        }
        let plan_text = plan.unwrap_or_else(|| t!("account.plan_unknown").into_owned());
        let header = Paragraph::new(vec![
            Line::from(account_line),
            Line::from(t!("account.plan", plan = plan_text).into_owned()),
        ]);
        f.render_widget(header, sections[0]);

        self.draw_window(
            f,
            sections[1],
            t!("account.five_hour").into_owned(),
            five.as_ref(),
        );
        self.draw_window(
            f,
            sections[2],
            t!("account.weekly").into_owned(),
            weekly.as_ref(),
        );

        let mut lines: Vec<Line> = Vec::new();
        if let Some(n) = note {
            lines.push(n);
        }
        if let Some(c) = credits {
            lines.push(Line::from(Span::styled(
                t!("account.reset_credits").into_owned(),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(
                t!("account.reset_credits_value", count = c).into_owned(),
            ));
            for (idx, detail) in credit_details.iter().enumerate() {
                lines.push(Line::from(
                    t!(
                        "account.reset_credit_item",
                        n = idx + 1,
                        expires = format_credit_datetime(&detail.expires_at),
                        granted = format_credit_datetime(&detail.granted_at)
                    )
                    .into_owned(),
                ));
            }
            if credit_details.is_empty() {
                lines.push(
                    Line::from(t!("account.reset_credits_hint").into_owned()).fg(Color::DarkGray),
                );
            }
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), sections[3]);
    }

    /// A neutral summary for an agent header / add row (no live identity, so an
    /// un-added agent never looks "signed in").
    fn draw_agent_summary(&self, f: &mut Frame, area: Rect, focused: bool, agent: Agent) {
        let data = self.account.agent(agent);
        let block = self.focus_block(agent_name(agent).to_string(), focused);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines = vec![Line::from(
            t!("account.summary_count", count = data.slots.len()).into_owned(),
        )];
        if let Some(email) = data
            .slots
            .iter()
            .find(|s| s.active)
            .and_then(|s| s.meta.email.clone())
        {
            lines.push(Line::from(
                t!("account.summary_active", email = email).into_owned(),
            ));
        }
        lines.push(Line::from(t!("account.summary_add_hint").into_owned()).fg(Color::DarkGray));
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    }

    /// Draw one rate-limit window as a labelled gauge (or a dash when absent).
    fn draw_window(&self, f: &mut Frame, area: Rect, label: String, window: Option<&RateWindow>) {
        match window {
            Some(w) => {
                let used = w.used_percent.clamp(0.0, 100.0);
                let text = t!(
                    "account.window_line",
                    label = &label,
                    used = format!("{used:.0}"),
                    left = format!("{:.0}", w.remaining_percent()),
                    reset = fmt_reset(w.resets_at)
                )
                .into_owned();
                let gauge = Gauge::default()
                    .block(Block::default().borders(Borders::ALL).title(label))
                    .gauge_style(Style::default().fg(severity_color(used)))
                    .ratio(used / 100.0)
                    .label(text);
                f.render_widget(gauge, area);
            }
            None => {
                let para = Paragraph::new(format!("{label}: —"))
                    .block(Block::default().borders(Borders::ALL));
                f.render_widget(para, area);
            }
        }
    }

    fn draw_settings(&self, f: &mut Frame, area: Rect) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(area);
        let active_category = self.active_setting_category();
        let category_focused = self.focus_content && self.settings_focus == SettingsFocus::Category;
        let detail_focused = self.focus_content && self.settings_focus == SettingsFocus::Detail;
        let category_lines = SETTING_CATEGORIES
            .iter()
            .enumerate()
            .map(|(idx, category)| {
                let count = if idx == 0 {
                    self.settings.len()
                } else {
                    self.settings
                        .iter()
                        .filter(|row| setting_category_index(row.key) == idx)
                        .count()
                };
                let text = t!(
                    "settings.category_line",
                    name = t!(category.key).into_owned(),
                    count = count
                )
                .into_owned();
                let style = if idx == self.settings_category_idx {
                    self.selection_style()
                } else {
                    Style::default()
                };
                Line::from(Span::styled(text, style))
            })
            .collect::<Vec<_>>();
        f.render_widget(
            Paragraph::new(category_lines)
                .block(self.focus_block(t!("settings.categories").into_owned(), category_focused)),
            cols[0],
        );

        let active_indices = self.setting_indices_in_active_category();
        if active_indices.is_empty() {
            f.render_widget(
                Paragraph::new(t!("settings.empty_category").into_owned())
                    .wrap(Wrap { trim: true })
                    .block(self.focus_block(
                        format!(
                            "{} — {}",
                            t!(active_category.key),
                            t!(active_category.desc_key)
                        ),
                        detail_focused,
                    )),
                cols[1],
            );
            return;
        }

        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(8), Constraint::Length(9)])
            .split(cols[1]);
        let visible_indices = self.visible_setting_indices(&active_indices, right[0].height);
        let rows = visible_indices.iter().map(|i| {
            let r = &self.settings[*i];
            let style = if *i == self.settings_idx {
                self.selection_style()
            } else {
                Style::default()
            };
            Row::new([Cell::from(r.key), Cell::from(r.value.clone())]).style(style)
        });
        let title = format!(
            "{} — {}",
            t!(active_category.key),
            t!(active_category.desc_key)
        );
        let title = if let Some(query) = self.settings_search.as_ref() {
            format!("{title} / {query}")
        } else if visible_indices.len() < active_indices.len() {
            let first = visible_indices
                .first()
                .and_then(|idx| active_indices.iter().position(|i| i == idx))
                .unwrap_or(0)
                + 1;
            let last = first + visible_indices.len().saturating_sub(1);
            format!("{title} {first}-{last}/{}", active_indices.len())
        } else {
            title
        };
        let table = Table::new(rows, [Constraint::Min(36), Constraint::Length(14)])
            .header(
                Row::new([
                    t!("table.setting").into_owned(),
                    t!("table.value").into_owned(),
                ])
                .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .block(self.focus_block(title, detail_focused));
        f.render_widget(table, right[0]);

        let detail = Paragraph::new(self.settings_detail_lines())
            .wrap(Wrap { trim: false })
            .block(self.focus_block(t!("settings.detail_title"), detail_focused));
        f.render_widget(detail, right[1]);
    }

    fn settings_detail_lines(&self) -> Vec<Line<'static>> {
        let Some(row) = self.settings.get(self.settings_idx) else {
            return vec![Line::from(t!("settings.empty_category").into_owned())];
        };
        let default = config::default_value(row.key).unwrap_or_else(|_| "-".to_string());
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    t!("settings.detail_key").into_owned(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(" {}", row.key)),
            ]),
            Line::from(vec![
                Span::styled(
                    t!("settings.detail_value").into_owned(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(" {}", row.value)),
            ]),
            Line::from(vec![
                Span::styled(
                    t!("settings.detail_default").into_owned(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(" {default}")),
            ]),
            Line::from(setting_desc(row.key)),
            Line::from(t!("settings.detail_help").into_owned()).fg(Color::DarkGray),
        ];
        if let Some(buf) = self.settings_edit_buf.as_ref() {
            lines.push(Line::from(vec![
                Span::styled(
                    t!("settings.detail_editing").into_owned(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(" {buf}")),
            ]));
        }
        if row.key.starts_with("theme.") {
            lines.push(self.theme_preview_line());
        }
        lines
    }

    fn theme_preview_line(&self) -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!(" {} ", t!("settings.detail_preview")),
                self.selection_style(),
            ),
            Span::raw("  "),
            Span::styled("Codex", Style::default().fg(self.theme.codex)),
            Span::raw(" / "),
            Span::styled("Claude", Style::default().fg(self.theme.claude)),
            Span::raw("  "),
            Span::styled("focus", Style::default().fg(self.theme.focus_border)),
        ])
    }

    fn draw_status(&self, f: &mut Frame, area: Rect) {
        let para =
            Paragraph::new(Span::raw(&self.status)).block(Block::default().borders(Borders::ALL));
        f.render_widget(para, area);
    }
}

fn health_table_row(r: HealthRow) -> Row<'static> {
    let (sym, color) = status_style(&r.status);
    Row::new([
        Cell::from(r.label),
        Cell::from(sym).style(Style::default().fg(color)),
        Cell::from(r.detail),
    ])
}

/// Display name for an agent in the Account tab.
fn agent_name(agent: Agent) -> &'static str {
    match agent {
        Agent::Codex => "Codex",
        Agent::Claude => "Claude",
    }
}

fn overview_limit_line(label: &str, window: Option<&RateWindow>, note: &Option<String>) -> String {
    let label = format!("{label:<11}");
    if let Some(window) = window {
        let left = window.remaining_percent().round().clamp(0.0, 100.0) as u64;
        format!(
            "{label} {} {}",
            overview_limit_bar(left, 12),
            t!("overview.limit_left", pct = left).into_owned()
        )
    } else {
        let note = note
            .clone()
            .unwrap_or_else(|| t!("overview.limit_no_account").into_owned());
        format!("{label} {note}")
    }
}

fn overview_limit_bar(left_percent: u64, width: usize) -> String {
    let filled = ((left_percent.min(100) as f64 / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// Gauge color by how used a window is: green < 70% < yellow < 90% < red.
fn severity_color(used_percent: f64) -> Color {
    if used_percent < 70.0 {
        Color::Green
    } else if used_percent < 90.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// A " · resets in 2h08m" suffix from a unix-seconds reset time (empty when
/// unknown), used to annotate a window gauge.
fn fmt_reset(resets_at: Option<i64>) -> String {
    let Some(ts) = resets_at else {
        return String::new();
    };
    let secs = ts - chrono::Utc::now().timestamp();
    if secs <= 0 {
        return t!("account.reset_suffix", when = "now").into_owned();
    }
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    let when = if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m")
    };
    t!("account.reset_suffix", when = when).into_owned()
}

fn format_credit_datetime(value: &str) -> String {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| value.to_string())
}

/// Run `ai-handoff autostart on|off` as a detached child (no console window),
/// so the OS logon entry is actually registered/removed — not just the config.
fn apply_autostart(on: bool) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("autostart")
        .arg(if on { "on" } else { "off" })
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "autostart command exited with {status}"
        )))
    }
}

/// Translated name of an editable capsule field.
fn field_label(field: capsule_ops::CapField) -> String {
    let key = match field {
        capsule_ops::CapField::Goal => "field.goal",
        capsule_ops::CapField::NextPrompt => "field.next_prompt",
        capsule_ops::CapField::Remaining => "field.remaining",
        capsule_ops::CapField::Done => "field.done",
        capsule_ops::CapField::Risks => "field.risks",
    };
    t!(key).into_owned()
}

/// Translated consumption-state word.
fn state_label(state: &str) -> String {
    match state {
        "pending" => t!("state.pending").into_owned(),
        "in_progress" => t!("state.in_progress").into_owned(),
        "blocked" => t!("state.blocked").into_owned(),
        "needs_review" => t!("state.needs_review").into_owned(),
        "consumed" => t!("state.consumed").into_owned(),
        "archived" => t!("state.archived").into_owned(),
        other => other.to_string(),
    }
}

/// Flip membership of `key` in `set` (insert if absent, remove if present).
fn toggle<T: Eq + std::hash::Hash>(set: &mut HashSet<T>, key: T) {
    if !set.remove(&key) {
        set.insert(key);
    }
}

/// A bordered block whose outline is highlighted (thick + yellow) when focused —
/// this is the "외곽선" that shows which pane the user is in.
fn focus_block_with_color(
    title: impl Into<String>,
    focused: bool,
    focus_color: Color,
) -> Block<'static> {
    let (border_type, style) = if focused {
        (BorderType::Thick, Style::default().fg(focus_color))
    } else {
        (BorderType::Plain, Style::default().fg(Color::DarkGray))
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(style)
        .title(title.into())
}

/// Basename of a project id/path, truncated to fit the tree column.
fn project_label(id: &str) -> String {
    let base = id
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(id);
    truncate(base, 26)
}

fn source_label(id: &str) -> String {
    match id {
        "claude" => "Claude".to_string(),
        "codex" => "Codex".to_string(),
        other => other.to_string(),
    }
}

/// One-line capsule label: date + state + a short summary preview.
fn capsule_label(cap: &ai_handoff_core::dashboard::CapsuleSummary) -> String {
    let when = cap.created_at.get(..10).unwrap_or(&cap.created_at);
    let preview = if cap.summary_preview.is_empty() {
        "(no summary)"
    } else {
        cap.summary_preview.as_str()
    };
    format!(
        "{when} [{}] {}",
        state_label(&cap.state),
        truncate(preview, 30)
    )
}

/// Truncate to `max` chars, appending '…' when shortened.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

fn nonempty_lines(text: &str, max: usize) -> Vec<String> {
    let mut lines = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(max)
        .map(|line| truncate(line, 160))
        .collect::<Vec<_>>();
    if lines.is_empty() && !text.trim().is_empty() {
        lines.push(truncate(text.trim(), 160));
    }
    lines
}

fn status_style(status: &CheckStatus) -> (&'static str, Color) {
    match status {
        CheckStatus::Ok => ("ok", Color::Green),
        CheckStatus::Warning => ("warn", Color::Yellow),
        CheckStatus::Error => ("error", Color::Red),
        CheckStatus::Missing => ("missing", Color::DarkGray),
        CheckStatus::Unknown => ("?", Color::DarkGray),
    }
}

fn setting_category_index(key: &str) -> usize {
    if key.starts_with("triggers.") {
        2
    } else if key.starts_with("capsule.") {
        3
    } else if key.starts_with("paths.") {
        4
    } else if key.starts_with("theme.") || matches!(key, "language" | "statusline.show") {
        5
    } else if key.starts_with("security.") {
        6
    } else if key.starts_with("agents.") {
        7
    } else if key.starts_with("autostart.") {
        1
    } else {
        8
    }
}

fn is_selection_color_key(key: &str) -> bool {
    matches!(key, "theme.selection_bg_color" | "theme.selection_fg_color")
}

fn settings_table_row_capacity(table_height: u16) -> usize {
    table_height.saturating_sub(3).max(1) as usize
}

/// Compact token count with a K/M/B unit, e.g. 1_021_205_181 -> "1.02B".
/// Precision adapts: <10 -> 2dp, <100 -> 1dp, else integer.
fn human_tokens(n: u64) -> String {
    let f = n as f64;
    let (val, suffix) = if f >= 1e9 {
        (f / 1e9, "B")
    } else if f >= 1e6 {
        (f / 1e6, "M")
    } else if f >= 1e3 {
        (f / 1e3, "K")
    } else {
        return n.to_string();
    };
    let s = if val >= 100.0 {
        format!("{val:.0}")
    } else if val >= 10.0 {
        format!("{val:.1}")
    } else {
        format!("{val:.2}")
    };
    format!("{s}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn test_app() -> App {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = ai_handoff_core::dashboard::dashboard_snapshot_for(dir.path(), dir.path());
        let usage = UsageView::from_events(&[]);
        let cfg = ai_handoff_core::config::Config::default();
        let path = dir.path().join("config.toml");
        // leak the tempdir so the path stays valid for the test body
        std::mem::forget(dir);
        App::new(snapshot, usage, settings_rows(&cfg), path)
    }

    #[test]
    fn top_tab_q_arms_quit_not_back() {
        let mut app = test_app();
        app.on_key(key(KeyCode::Char('2'))); // Overview -> Capsule
        app.on_key(key(KeyCode::Char('6'))); // Capsule -> Settings
        assert_eq!(app.tab, Tab::Settings);

        // On a top tab, q does NOT go back to a previous tab — it arms a quit.
        app.on_key(key(KeyCode::Char('q')));
        assert_eq!(app.tab, Tab::Settings, "must stay, not step back");
        assert!(app.confirm_quit);
        assert!(!app.should_quit());

        app.on_key(key(KeyCode::Char('q'))); // second q quits
        assert!(app.should_quit());
    }

    #[test]
    fn q_in_content_returns_to_tab_bar_then_arms_quit() {
        let mut app = test_app();
        app.on_key(key(KeyCode::Char('6'))); // Settings tab bar
        app.on_key(key(KeyCode::Down)); // descend into content
        assert!(app.focus_content);

        app.on_key(key(KeyCode::Char('q'))); // leave content -> tab bar (not quit)
        assert!(!app.focus_content);
        assert!(!app.confirm_quit);
        assert_eq!(app.tab, Tab::Settings);

        app.on_key(key(KeyCode::Char('q'))); // now on the tab bar -> arm quit
        assert!(app.confirm_quit);
        assert!(!app.should_quit());
    }

    #[test]
    fn q_on_overview_arms_then_quits() {
        let mut app = test_app();
        assert_eq!(app.tab, Tab::Overview);
        app.on_key(key(KeyCode::Char('q')));
        assert!(!app.should_quit(), "first q must only arm");
        assert!(app.confirm_quit);
        // language-neutral: status shows the quit hint (whatever the locale)
        assert_eq!(app.status, quit_hint());
        app.on_key(key(KeyCode::Char('q')));
        assert!(app.should_quit(), "second q quits");
    }

    #[test]
    fn esc_behaves_like_q_on_overview() {
        let mut app = test_app();
        app.on_key(key(KeyCode::Esc));
        assert!(!app.should_quit());
        app.on_key(key(KeyCode::Esc));
        assert!(app.should_quit());
    }

    #[test]
    fn other_key_disarms_quit_confirmation() {
        let mut app = test_app();
        app.on_key(key(KeyCode::Char('q'))); // arm on Overview
        assert!(app.confirm_quit);
        app.on_key(key(KeyCode::Tab)); // any other key disarms + navigates
        assert!(!app.confirm_quit);
        assert_eq!(app.tab, Tab::Capsule);
        // q on the new top tab arms again (no back-through-tabs)
        app.on_key(key(KeyCode::Char('q')));
        assert!(!app.should_quit());
        assert!(app.confirm_quit);
        assert_eq!(app.tab, Tab::Capsule);
    }

    fn test_slot(label: &str, active: bool) -> account::AccountSlot {
        account::AccountSlot {
            meta: account::AccountMeta {
                schema_version: 1,
                agent: "codex".into(),
                label: label.into(),
                email: Some(label.into()),
                plan_hint: None,
                account_id: None,
                workspace_id: None,
                created_at: None,
                last_verified_at: None,
                source: None,
            },
            dir: std::path::PathBuf::from(format!("/p/{label}")),
            active,
        }
    }

    /// Build an app with an injected Codex account (one slot) for nav tests.
    fn account_app() -> App {
        let mut app = test_app();
        app.account.codex = AgentAccount {
            status: None,
            slots: vec![
                test_slot("dev@example.com", true),
                test_slot("alt@example.com", false),
            ],
        };
        app
    }

    #[test]
    fn account_rows_list_agents_slots_and_add() {
        let app = account_app();
        let rows = app.acc_rows();
        // Codex: header + 2 slots + add (4) ; Claude: header + add (2) = 6.
        assert_eq!(rows.len(), 6);
        assert!(matches!(rows[0].target, AccTarget::Header(Agent::Codex)));
        assert!(matches!(rows[1].target, AccTarget::Slot(Agent::Codex, 0)));
        assert!(matches!(rows[3].target, AccTarget::Add(Agent::Codex)));
        assert!(matches!(rows[4].target, AccTarget::Header(Agent::Claude)));
        assert!(rows[1].label.contains("dev@example.com"));
        // The active slot is annotated.
        assert!(rows[1].label.contains(&t!("account.active").into_owned()));
    }

    #[test]
    fn account_rows_show_email_when_slot_key_is_account_id() {
        let mut app = test_app();
        app.account.codex = AgentAccount {
            status: None,
            slots: vec![account::AccountSlot {
                meta: account::AccountMeta {
                    schema_version: 1,
                    agent: "codex".into(),
                    label: "acc-work".into(),
                    email: Some("work@example.com".into()),
                    plan_hint: Some("business".into()),
                    account_id: Some("acc-work".into()),
                    workspace_id: None,
                    created_at: None,
                    last_verified_at: None,
                    source: None,
                },
                dir: std::path::PathBuf::from("/p/acc-work"),
                active: false,
            }],
        };

        let rows = app.acc_rows();
        assert!(rows[1].label.contains("work@example.com"));
        assert!(!rows[1].label.contains("acc-work"));
    }

    #[test]
    fn account_action_labels_distinguish_app_switch_from_cli_launch() {
        assert_eq!(t!("account.btn_switch", locale = "ko"), "[s] 앱 전환");
        assert_eq!(t!("account.btn_launch", locale = "ko"), "[l] CLI로 실행");
        assert!(t!("hint.account_detail", locale = "ko").contains("s 앱 전환"));
        assert!(t!("hint.account_detail", locale = "ko").contains("l CLI로 실행"));
    }

    #[test]
    fn account_nav_enters_detail_on_a_slot_and_back() {
        let mut app = account_app();
        app.on_key(key(KeyCode::Char('4'))); // -> Account tab bar
        assert_eq!(app.tab, Tab::Account);
        assert!(!app.focus_content);
        app.on_key(key(KeyCode::Down)); // descend into the tree
        assert!(app.focus_content);
        assert_eq!(app.acc_focus, AccFocus::Tree);
        app.on_key(key(KeyCode::Down)); // move onto the first Codex slot
        assert!(matches!(
            app.acc_target(),
            Some(AccTarget::Slot(Agent::Codex, 0))
        ));
        app.on_key(key(KeyCode::Enter)); // cross into the detail pane
        assert_eq!(app.acc_focus, AccFocus::Detail);
        app.on_key(key(KeyCode::Left)); // back to the tree
        assert_eq!(app.acc_focus, AccFocus::Tree);
    }

    #[test]
    fn window_line_interpolates_in_all_locales() {
        for loc in ["en", "ko", "ja", "zh"] {
            let s = t!(
                "account.window_line",
                locale = loc,
                label = "5h",
                used = "18",
                left = "82",
                reset = " · resets in 2h"
            )
            .into_owned();
            assert!(s.contains("82"), "{loc}: {s}");
            assert!(s.contains("resets in 2h"), "{loc} dropped reset: {s}");
            assert!(!s.contains("%{"), "{loc} leaked a placeholder: {s}");
        }
    }

    #[test]
    fn account_delete_needs_confirm() {
        let mut app = account_app();
        app.tab = Tab::Account;
        app.focus_content = true;
        app.acc_sel = 1; // first Codex slot
        app.acc_focus = AccFocus::Detail;
        app.on_key(key(KeyCode::Char('d'))); // arm
        assert!(app.acc_confirm_delete);
        app.on_key(key(KeyCode::Char('x'))); // any other key disarms
        assert!(!app.acc_confirm_delete);
    }

    #[test]
    fn account_a_queues_add_and_l_queues_launch() {
        let mut app = account_app();
        app.tab = Tab::Account;
        app.focus_content = true;
        app.acc_focus = AccFocus::Detail;
        app.acc_sel = 1; // first Codex slot (dev@example.com)

        // 'l' on a slot queues a launch for that slot.
        app.on_key(key(KeyCode::Char('l')));
        assert_eq!(
            app.pending,
            Some(Pending::Launch(Agent::Codex, "dev@example.com".into()))
        );

        // 'a' queues an OAuth add for the selected agent.
        app.pending = None;
        app.on_key(key(KeyCode::Char('a')));
        assert_eq!(app.pending, Some(Pending::AddAccount(Agent::Codex)));
    }

    #[test]
    fn human_tokens_uses_k_m_b_units() {
        assert_eq!(human_tokens(999), "999");
        assert_eq!(human_tokens(12_300), "12.3K");
        assert_eq!(human_tokens(991_010_690), "991M");
        assert_eq!(human_tokens(1_021_205_181), "1.02B");
        assert_eq!(human_tokens(2_012_215_871), "2.01B");
    }

    #[test]
    fn source_split_reads_claude_and_codex() {
        use ai_handoff_usage::model::{Source, Tokens, UsageEvent};
        let events = vec![
            UsageEvent {
                source: Source::Claude,
                project: "p".into(),
                session: "s".into(),
                model: "claude-opus-4-8".into(),
                day: "2026-06-17".into(),
                tokens: Tokens {
                    input: 10,
                    ..Default::default()
                },
            },
            UsageEvent {
                source: Source::Codex,
                project: "p".into(),
                session: "s".into(),
                model: "gpt-5.5".into(),
                day: "2026-06-17".into(),
                tokens: Tokens {
                    input: 4,
                    ..Default::default()
                },
            },
        ];
        let dir = tempfile::tempdir().unwrap();
        let snapshot = ai_handoff_core::dashboard::dashboard_snapshot_for(dir.path(), dir.path());
        let app = App::new(
            snapshot,
            UsageView::from_events(&events),
            vec![],
            dir.path().join("config.toml"),
        );
        assert_eq!(app.source_split(), (10, 4));
    }

    #[test]
    fn usage_summary_lines_show_total_and_source_split() {
        use ai_handoff_usage::model::{Source, Tokens, UsageEvent};
        let events = vec![
            UsageEvent {
                source: Source::Claude,
                project: "p".into(),
                session: "s".into(),
                model: "claude-opus-4-8".into(),
                day: "2026-06-17".into(),
                tokens: Tokens {
                    input: 10,
                    ..Default::default()
                },
            },
            UsageEvent {
                source: Source::Codex,
                project: "p".into(),
                session: "s".into(),
                model: "gpt-5.5".into(),
                day: "2026-06-17".into(),
                tokens: Tokens {
                    input: 4,
                    ..Default::default()
                },
            },
        ];
        let dir = tempfile::tempdir().unwrap();
        let snapshot = ai_handoff_core::dashboard::dashboard_snapshot_for(dir.path(), dir.path());
        let app = App::new(
            snapshot,
            UsageView::from_events(&events),
            vec![],
            dir.path().join("config.toml"),
        );

        let lines = app.usage_summary_lines();

        assert!(lines.iter().any(|line| line == "Total 14 tokens"));
        assert!(lines.iter().any(|line| line == "Claude 10 tokens"));
        assert!(lines.iter().any(|line| line == "Codex 4 tokens"));
    }

    #[test]
    fn overview_limit_lines_show_active_agent_windows() {
        let mut app = test_app();
        app.account.claude = AgentAccount {
            status: None,
            slots: vec![account::AccountSlot {
                meta: account::AccountMeta {
                    schema_version: 1,
                    agent: "claude".into(),
                    label: "claude-active".into(),
                    email: Some("claude@example.com".into()),
                    plan_hint: Some("pro".into()),
                    account_id: None,
                    workspace_id: None,
                    created_at: None,
                    last_verified_at: None,
                    source: None,
                },
                dir: std::path::PathBuf::from("/p/claude-active"),
                active: true,
            }],
        };
        app.account.codex = AgentAccount {
            status: None,
            slots: vec![account::AccountSlot {
                meta: account::AccountMeta {
                    schema_version: 1,
                    agent: "codex".into(),
                    label: "codex-active".into(),
                    email: Some("codex@example.com".into()),
                    plan_hint: Some("team".into()),
                    account_id: None,
                    workspace_id: None,
                    created_at: None,
                    last_verified_at: None,
                    source: None,
                },
                dir: std::path::PathBuf::from("/p/codex-active"),
                active: true,
            }],
        };
        app.acc_usage.insert(
            App::usage_key(Agent::Claude, "claude-active"),
            UsageState::Loaded(crate::account_api::UsageData {
                five_hour: Some(RateWindow {
                    used_percent: 78.0,
                    window_minutes: 300,
                    resets_at: None,
                }),
                weekly: Some(RateWindow {
                    used_percent: 54.0,
                    window_minutes: 10080,
                    resets_at: None,
                }),
                ..Default::default()
            }),
        );
        app.acc_usage.insert(
            App::usage_key(Agent::Codex, "codex-active"),
            UsageState::Loaded(crate::account_api::UsageData {
                five_hour: Some(RateWindow {
                    used_percent: 90.0,
                    window_minutes: 300,
                    resets_at: None,
                }),
                weekly: Some(RateWindow {
                    used_percent: 30.0,
                    window_minutes: 10080,
                    resets_at: None,
                }),
                ..Default::default()
            }),
        );

        let lines = app.overview_limit_lines();

        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("Claude 5h"));
        assert!(lines[0].contains("22% left"));
        assert!(lines[1].starts_with("Claude week"));
        assert!(lines[1].contains("46% left"));
        assert!(lines[2].starts_with("Codex 5h"));
        assert!(lines[2].contains("10% left"));
        assert!(lines[3].starts_with("Codex week"));
        assert!(lines[3].contains("70% left"));
    }

    #[test]
    fn action_center_lines_show_positive_empty_state() {
        let mut app = test_app();
        for check in &mut app.snapshot.checks {
            check.status = CheckStatus::Ok;
            check.message = "ok".into();
        }
        app.snapshot.capsules.items.clear();
        app.snapshot.capsules.pending_count = 0;

        let lines = app.action_center_lines();

        assert_eq!(
            lines,
            vec![
                "ok All integrations normal".to_string(),
                "ok No pending capsule".to_string(),
                "ok Automatic handoff standing by".to_string(),
            ]
        );
    }

    #[test]
    fn integration_status_lines_include_check_status_and_detail() {
        let app = test_app();

        let lines = app.integration_status_lines();

        assert!(lines.iter().any(|line| line.contains("Daemon")));
        assert!(lines.iter().any(|line| line.contains("Codex hooks")));
        assert!(lines.iter().any(|line| line.contains(":")));
    }

    #[test]
    fn overview_usage_integration_and_settings_labels_translate() {
        for locale in ["en", "ko", "ja", "zh"] {
            for key in [
                "overview.actions",
                "overview.current_project",
                "overview.recent_activity",
                "overview.agent_limits",
                "overview.limit_left",
                "overview.limit_loading",
                "overview.limit_no_account",
                "overview.limit_error",
                "overview.all_integrations_normal",
                "usage.total_tokens",
                "usage.estimate_line",
                "integration.repair_actions",
                "integration.recent_diagnostics",
                "integration.hooks",
                "integration.repair_center",
                "integration.doctor_title",
                "integration.logs_title",
                "settings.category.all",
                "settings.category.automation",
                "settings.category.triggers",
                "settings.category.capsule",
                "settings.category.paths",
                "settings.category.display",
                "settings.category.security",
                "settings.category.agents",
                "settings.category.advanced",
                "settings.detail_title",
                "settings.detail_key",
                "settings.detail_value",
                "settings.detail_default",
                "settings.detail_help",
                "settings.detail_editing",
                "settings.detail_preview",
                "setting.capsule_format",
                "setting.capsule_next_prompt_max_items",
                "setting.capsule_remaining_max_items",
                "setting.capsule_done_max_items",
                "setting.capsule_risks_max_items",
                "setting.theme_preset",
                "setting.theme_codex_color",
                "setting.theme_claude_color",
                "setting.theme_focus_border_color",
                "setting.theme_selection_bg_color",
                "setting.theme_selection_fg_color",
                "status.settings_search",
                "status.settings_search_done",
                "status.settings_editing_color",
                "status.settings_edit_cancelled",
                "state.in_progress",
                "state.blocked",
                "state.needs_review",
                "state.archived",
            ] {
                let value = t!(
                    key,
                    locale = locale,
                    tokens = "1",
                    cost = "1.00",
                    pct = "1",
                    err = "x"
                )
                .into_owned();
                assert!(!value.is_empty(), "{locale}:{key}");
                assert!(!value.starts_with("translation missing"), "{locale}:{key}");
            }
        }
    }

    #[test]
    fn settings_categories_follow_design_doc_order() {
        let keys: Vec<&str> = SETTING_CATEGORIES.iter().map(|c| c.key).collect();
        assert_eq!(
            keys,
            vec![
                "settings.category.all",
                "settings.category.automation",
                "settings.category.triggers",
                "settings.category.capsule",
                "settings.category.paths",
                "settings.category.display",
                "settings.category.security",
                "settings.category.agents",
                "settings.category.advanced",
            ]
        );
    }

    #[test]
    fn tab_titles_translate_with_locale() {
        assert_eq!(t!("tab.overview", locale = "ko"), "개요");
        assert_eq!(t!("tab.usage", locale = "ko"), "사용량");
        assert_eq!(t!("tab.integration", locale = "ko"), "연동");
        assert_eq!(t!("tab.account", locale = "ja"), "アカウント");
        assert_eq!(t!("tab.settings", locale = "zh"), "设置");
        assert_eq!(t!("tab.capsule", locale = "en"), "Capsule");
    }

    #[test]
    fn tab_cycles_forward_and_number_keys_jump() {
        let mut app = test_app();
        assert_eq!(app.tab, Tab::Overview);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.tab, Tab::Capsule);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.tab, Tab::Usage);
        app.on_key(key(KeyCode::Char('5')));
        assert_eq!(app.tab, Tab::Integration);
        app.on_key(key(KeyCode::Char('6')));
        assert_eq!(app.tab, Tab::Settings);
        app.on_key(key(KeyCode::Char('1')));
        assert_eq!(app.tab, Tab::Overview);
    }

    #[test]
    fn entering_a_tab_does_not_auto_activate_its_content() {
        let mut app = test_app();
        app.on_key(key(KeyCode::Char('6'))); // -> Settings, on the tab bar
        assert_eq!(app.tab, Tab::Settings);
        assert!(!app.focus_content, "Settings must not auto-enter edit mode");
        // Space at the tab-bar level descends but must not edit the first row.
        app.on_key(key(KeyCode::Char(' ')));
        assert!(app.focus_content);
        assert_eq!(app.settings[0].value, "true", "descend must not toggle");
        assert_eq!(app.settings_focus, SettingsFocus::Category);
        // A second Space enters the right detail pane; it still must not edit.
        app.on_key(key(KeyCode::Char(' ')));
        assert_eq!(app.settings_focus, SettingsFocus::Detail);
        assert_eq!(app.settings[0].value, "true");
        // Now Space in the detail pane actually edits.
        app.on_key(key(KeyCode::Char(' ')));
        assert_eq!(app.settings[0].value, "false");
    }

    #[test]
    fn down_enters_content_without_moving_selection() {
        let mut app = test_app();
        app.on_key(key(KeyCode::Char('6'))); // -> Settings tab bar
        app.on_key(key(KeyCode::Down)); // descend
        assert!(app.focus_content);
        assert_eq!(
            app.settings_idx, 0,
            "the descending Down must not also move"
        );
    }

    #[test]
    fn settings_navigation_and_edit_persist() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_focus = SettingsFocus::Detail;
        // first row is triggers.five_hour.enabled (bool, default true)
        let first_key = app.settings[0].key;
        app.on_key(key(KeyCode::Char(' ')));
        assert_eq!(app.settings[0].value, "false");
        // language-neutral: the saved status interpolates the key in any locale
        assert!(app.status.contains(first_key));
        // the write actually landed
        let cfg = ai_handoff_core::config::load_from(&app.config_path);
        let val = ai_handoff_core::config::get_value(&cfg, first_key).unwrap();
        assert_eq!(val, "false");
    }

    #[test]
    fn settings_status_shows_selected_description() {
        let mut app = test_app();
        app.on_key(key(KeyCode::Char('6'))); // -> Settings tab bar
        app.on_key(key(KeyCode::Down)); // descend; status = desc of row 0
        assert!(app.focus_content);
        assert_eq!(app.settings_idx, 0);
        assert_eq!(app.settings_focus, SettingsFocus::Category);
        app.on_key(key(KeyCode::Right)); // enter detail; status = desc of row 0
        assert_eq!(app.settings_focus, SettingsFocus::Detail);
        assert_eq!(app.status, setting_desc(app.settings[0].key));
        assert!(!app.status.is_empty());
        app.on_key(key(KeyCode::Down)); // move to row 1; description updates
        assert_eq!(app.settings_idx, 1);
        assert_eq!(app.status, setting_desc(app.settings[1].key));
    }

    #[test]
    fn settings_down_moves_selection() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_focus = SettingsFocus::Detail;
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.settings_idx, 1);
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.settings_idx, 0);
    }

    #[test]
    fn settings_enters_detail_and_esc_leaves_settings_content() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_focus = SettingsFocus::Category;
        app.settings_category_idx = 0;

        app.on_key(key(KeyCode::Down));
        assert_eq!(app.settings_category_idx, 1);
        assert_eq!(app.settings_focus, SettingsFocus::Category);

        app.on_key(key(KeyCode::Right));
        assert_eq!(app.settings_focus, SettingsFocus::Detail);
        assert!(app.settings[app.settings_idx].key.starts_with("autostart."));

        app.on_key(key(KeyCode::Esc));
        assert!(!app.focus_content);
        assert_eq!(app.settings_focus, SettingsFocus::Detail);
    }

    #[test]
    fn usage_and_integration_focus_moves_between_subpanes() {
        let mut app = test_app();
        app.tab = Tab::Usage;
        app.focus_content = true;
        app.usage_focus = UsageFocus::Chart;
        app.on_key(key(KeyCode::Right));
        assert_eq!(app.usage_focus, UsageFocus::Details);
        app.on_key(key(KeyCode::Left));
        assert_eq!(app.usage_focus, UsageFocus::Chart);

        app.tab = Tab::Integration;
        app.integration_focus = IntegrationFocus::Status;
        app.on_key(key(KeyCode::Right));
        assert_eq!(app.integration_focus, IntegrationFocus::Repair);
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.integration_focus, IntegrationFocus::Hooks);
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.integration_focus, IntegrationFocus::Diagnostics);
    }

    #[test]
    fn usage_g_cycles_breakdown_modes() {
        let mut app = test_app();
        app.tab = Tab::Usage;
        app.focus_content = true;

        assert_eq!(app.usage_mode, UsageViewMode::Summary);
        app.on_key(key(KeyCode::Char('g')));
        assert_eq!(app.usage_mode, UsageViewMode::Day);
        app.on_key(key(KeyCode::Char('g')));
        assert_eq!(app.usage_mode, UsageViewMode::Project);
        app.on_key(key(KeyCode::Char('g')));
        assert_eq!(app.usage_mode, UsageViewMode::Model);
        app.on_key(key(KeyCode::Char('g')));
        assert_eq!(app.usage_mode, UsageViewMode::Source);
        app.on_key(key(KeyCode::Char('g')));
        assert_eq!(app.usage_mode, UsageViewMode::Summary);
    }

    #[test]
    fn integration_keys_open_pages_and_back_returns_home_first() {
        let mut app = test_app();
        app.tab = Tab::Integration;
        app.focus_content = true;

        app.on_key(key(KeyCode::Char('d')));
        assert_eq!(app.integration_page, IntegrationPage::DoctorRun);
        assert!(app
            .integration_output
            .iter()
            .any(|line| line.contains("doctor")));

        app.on_key(key(KeyCode::Char('q')));
        assert!(app.focus_content);
        assert_eq!(app.integration_page, IntegrationPage::Home);

        app.on_key(key(KeyCode::Char('r')));
        assert_eq!(app.integration_page, IntegrationPage::RepairCenter);

        app.on_key(key(KeyCode::Char('q')));
        app.on_key(key(KeyCode::Char('l')));
        assert_eq!(app.integration_page, IntegrationPage::Logs);
    }

    #[test]
    fn repair_center_requires_confirmation_before_mutating_action() {
        let mut app = test_app();
        app.tab = Tab::Integration;
        app.focus_content = true;
        app.snapshot.codex_config.status = CheckStatus::Warning;
        app.snapshot.codex_config.message = "writable_roots=false, AI_HANDOFF_HOME=false".into();

        app.on_key(key(KeyCode::Char('r')));
        assert_eq!(app.integration_page, IntegrationPage::RepairCenter);
        assert!(!app.repair_confirm);

        app.on_key(key(KeyCode::Enter));
        assert!(app.repair_confirm);
        assert!(
            app.integration_output.is_empty(),
            "first Enter must arm confirmation, not execute"
        );
    }

    #[test]
    fn settings_all_category_contains_every_setting() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_category_idx = 0;

        assert_eq!(SETTING_CATEGORIES[0].key, "settings.category.all");
        assert_eq!(
            app.setting_indices_in_active_category().len(),
            app.settings.len()
        );

        app.settings_focus = SettingsFocus::Detail;
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.settings_idx, 1);
    }

    #[test]
    fn settings_table_viewport_keeps_selected_row_visible() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_focus = SettingsFocus::Detail;
        app.settings_category_idx = 0;
        app.settings_idx = app.settings.len() - 1;

        let all = app.setting_indices_in_active_category();
        let visible = app.visible_setting_indices(&all, 8);

        assert!(visible.contains(&app.settings_idx));
        assert!(!visible.contains(&0));
    }

    #[test]
    fn settings_left_edits_previous_value_without_leaving_detail() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_focus = SettingsFocus::Detail;
        let before = app.settings[app.settings_idx].value.clone();

        app.on_key(key(KeyCode::Left));

        assert_eq!(app.settings_focus, SettingsFocus::Detail);
        assert!(app.focus_content);
        assert_ne!(app.settings[app.settings_idx].value, before);
    }

    #[test]
    fn selection_color_cycle_skips_low_contrast_pairings() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_focus = SettingsFocus::Detail;
        app.settings_idx = app
            .settings
            .iter()
            .position(|row| row.key == "theme.selection_fg_color")
            .unwrap();

        app.on_key(key(KeyCode::Char(' ')));

        assert!(!app.status.contains("contrast"), "{}", app.status);
        assert_ne!(app.settings[app.settings_idx].value, "black");
        assert_ne!(app.theme.selection_fg, Color::Rgb(0, 0, 0));
    }

    #[test]
    fn selection_text_color_cycle_offers_more_than_blue_and_black() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_focus = SettingsFocus::Detail;

        let bg_idx = app
            .settings
            .iter()
            .position(|row| row.key == "theme.selection_bg_color")
            .unwrap();
        app.settings_idx = bg_idx;
        app.commit_setting_value(&app.settings[bg_idx].clone(), "white".to_string());

        let fg_idx = app
            .settings
            .iter()
            .position(|row| row.key == "theme.selection_fg_color")
            .unwrap();
        app.settings_idx = fg_idx;
        app.commit_setting_value(&app.settings[fg_idx].clone(), "blue".to_string());

        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..5 {
            app.on_key(key(KeyCode::Right));
            seen.insert(app.settings[fg_idx].value.clone());
        }

        assert!(seen.contains("magenta"), "{seen:?}");
        assert!(seen.contains("red"), "{seen:?}");
        assert!(seen.contains("green"), "{seen:?}");
    }

    #[test]
    fn settings_search_filters_and_keeps_filter_after_enter() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_focus = SettingsFocus::Detail;

        app.on_key(key(KeyCode::Char('/')));
        for c in "theme".chars() {
            app.on_key(key(KeyCode::Char(c)));
        }

        let keys = app
            .setting_indices_in_active_category()
            .into_iter()
            .map(|idx| app.settings[idx].key)
            .collect::<Vec<_>>();
        assert!(!keys.is_empty());
        assert!(keys.iter().all(|key| key.starts_with("theme.")));

        app.on_key(key(KeyCode::Enter));
        assert!(!app.settings_search_editing);
        assert_eq!(app.settings_search.as_deref(), Some("theme"));
        assert!(app.focus_content);
    }

    #[test]
    fn settings_reset_restores_default_value() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_focus = SettingsFocus::Detail;
        app.settings_idx = app
            .settings
            .iter()
            .position(|row| row.key == "theme.focus_border_color")
            .unwrap();

        app.commit_setting_value(&app.settings[app.settings_idx].clone(), "red".to_string());
        assert_eq!(app.settings[app.settings_idx].value, "red");

        app.on_key(key(KeyCode::Char('r')));
        assert_eq!(app.settings[app.settings_idx].value, "#FFA500");
        assert_eq!(app.theme.focus_border, Color::Rgb(255, 165, 0));
    }

    #[test]
    fn theme_settings_apply_to_selection_focus_and_agent_colors() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_focus = SettingsFocus::Detail;
        for (key_name, raw) in [
            ("theme.codex_color", "#112233"),
            ("theme.claude_color", "#445566"),
            ("theme.focus_border_color", "#778899"),
            ("theme.selection_bg_color", "white"),
            ("theme.selection_fg_color", "black"),
        ] {
            app.settings_idx = app
                .settings
                .iter()
                .position(|row| row.key == key_name)
                .unwrap();
            app.commit_setting_value(&app.settings[app.settings_idx].clone(), raw.to_string());
        }

        assert_eq!(app.agent_color(Agent::Codex), Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(app.agent_color(Agent::Claude), Color::Rgb(0x44, 0x55, 0x66));
        assert_eq!(app.theme.focus_border, Color::Rgb(0x77, 0x88, 0x99));
        assert_eq!(
            app.selection_style(),
            Style::default()
                .fg(Color::Rgb(0, 0, 0))
                .bg(Color::Rgb(255, 255, 255))
        );
    }

    #[test]
    fn theme_preset_without_overrides_changes_tui_theme() {
        let cfg = config::parse("[theme]\npreset = \"mono\"\n").unwrap();
        let theme = TuiTheme::from_config(&cfg);

        assert_eq!(theme.codex, Color::Rgb(255, 255, 255));
        assert_eq!(theme.claude, Color::Rgb(128, 128, 128));
        assert_eq!(theme.focus_border, Color::Rgb(255, 255, 255));
        assert_eq!(theme.selection_bg, Color::Rgb(255, 255, 255));
        assert_eq!(theme.selection_fg, Color::Rgb(0, 0, 0));
    }

    #[test]
    fn focused_panel_border_is_orange() {
        assert_eq!(TuiTheme::default().focus_border, Color::Rgb(255, 165, 0));
    }

    #[test]
    fn left_right_do_not_switch_tabs_in_settings() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
        app.settings_idx = 1; // mode row? regardless, Left edits, not tab-switch
        app.on_key(key(KeyCode::Left));
        assert_eq!(app.tab, Tab::Settings, "Left must edit, not leave Settings");
    }

    #[test]
    fn capsule_tree_expands_and_loads_content() {
        use ai_handoff_core::dashboard::{CapsuleList, CapsuleSummary};
        let dir = tempfile::tempdir().unwrap();
        let cap_path = dir.path().join("cap.json");
        std::fs::write(&cap_path, "{\"capsule body\":true}").unwrap();
        let snapshot = ai_handoff_core::dashboard::dashboard_snapshot_for(dir.path(), dir.path());
        let mut app = App::new(
            snapshot,
            UsageView::from_events(&[]),
            vec![],
            dir.path().join("config.toml"),
        );
        // Inject a one-capsule tree (agent expanded by default).
        app.cap_tree = crate::viewmodel::capsule_tree(&CapsuleList {
            items: vec![CapsuleSummary {
                capsule_id: "c1".into(),
                project_id: "proj-a".into(),
                created_at: "2026-06-25T01:01:01Z".into(),
                source_agent: "Codex".into(),
                target_agent: "ClaudeCode".into(),
                state: "pending".into(),
                summary_preview: "ship it".into(),
                path: cap_path.to_string_lossy().into_owned(),
            }],
            pending_count: 1,
            skipped: 0,
        });
        app.cap_expanded_agents.clear(); // start collapsed; expand via keys below
        app.tab = Tab::Capsule;
        app.focus_content = true;
        // Rows: [agent]. Expand it (Enter) -> [agent, project].
        app.on_key(key(KeyCode::Enter));
        // Move to the project, expand it -> capsule becomes visible.
        app.cap_sel = 1;
        app.on_key(key(KeyCode::Enter));
        // Move to the capsule; its file is read into the detail cache.
        app.cap_sel = 2;
        app.on_key(key(KeyCode::Down)); // clamps, stays on capsule, loads content
        let detail = app.cap_detail.as_ref().expect("capsule detail loaded");
        assert!(detail.raw.contains("capsule body"));
    }

    #[test]
    fn capsule_detail_actions_toggle_edit_and_delete() {
        use ai_handoff_core::capsule::{
            AgentKind, Capsule, Consumption, ConsumptionState, RedactionMeta, Session, Summary,
        };
        use ai_handoff_core::dashboard::{CapsuleList, CapsuleSummary};

        let dir = tempfile::tempdir().unwrap();
        let cap_path = dir.path().join("cap_1.json");
        let capsule = Capsule {
            schema_version: 2,
            capsule_id: "cap_1".into(),
            project_id: "projX".into(),
            created_at: "2026-06-25T12:00:00Z".into(),
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
                applied: true,
                ruleset: "default-v2".into(),
            },
            consumption: Consumption {
                state: ConsumptionState::Pending,
                consumed_by: None,
                consumed_at: None,
            },
        };
        std::fs::write(&cap_path, serde_json::to_vec_pretty(&capsule).unwrap()).unwrap();

        let snapshot = ai_handoff_core::dashboard::dashboard_snapshot_for(dir.path(), dir.path());
        let mut app = App::new(
            snapshot,
            UsageView::from_events(&[]),
            vec![],
            dir.path().join("config.toml"),
        );
        app.cap_tree = crate::viewmodel::capsule_tree(&CapsuleList {
            items: vec![CapsuleSummary {
                capsule_id: "cap_1".into(),
                project_id: "projX".into(),
                created_at: "2026-06-25T12:00:00Z".into(),
                source_agent: "Codex".into(),
                target_agent: "ClaudeCode".into(),
                state: "pending".into(),
                summary_preview: "old goal".into(),
                path: cap_path.to_string_lossy().into_owned(),
            }],
            pending_count: 1,
            skipped: 0,
        });
        app.cap_expanded_agents.clear();
        app.tab = Tab::Capsule;
        app.focus_content = true;

        // Expand agent (sel 0), then project (sel 1), then open the capsule.
        app.on_key(key(KeyCode::Enter));
        app.cap_sel = 1;
        app.on_key(key(KeyCode::Enter));
        app.cap_sel = 2;
        app.on_key(key(KeyCode::Right)); // capsule -> detail pane
        assert_eq!(app.cap_focus, CapFocus::Detail);

        // Toggle state: disk + in-memory both advance to the next state.
        app.on_key(key(KeyCode::Char('s')));
        assert_eq!(app.cap_tree[0].projects[0].capsules[0].state, "in_progress");
        let on_disk: Capsule = serde_json::from_slice(&std::fs::read(&cap_path).unwrap()).unwrap();
        assert_eq!(on_disk.consumption.state, ConsumptionState::InProgress);

        // Edit the goal: 'e' loads it, type, Enter saves.
        app.on_key(key(KeyCode::Char('e')));
        assert_eq!(app.cap_focus, CapFocus::Editing);
        assert_eq!(app.cap_edit_buf, "old goal");
        app.on_key(key(KeyCode::Char('!')));
        app.on_key(key(KeyCode::Enter));
        let on_disk: Capsule = serde_json::from_slice(&std::fs::read(&cap_path).unwrap()).unwrap();
        assert_eq!(on_disk.summary.goal, "old goal!");
        assert_eq!(app.cap_focus, CapFocus::Detail);

        // Delete needs a confirm; then the file and the tree entry are gone.
        app.on_key(key(KeyCode::Char('d')));
        assert!(app.cap_confirm_delete);
        app.on_key(key(KeyCode::Char('d')));
        assert!(!cap_path.exists());
        assert!(app.cap_tree.is_empty());
    }
}
