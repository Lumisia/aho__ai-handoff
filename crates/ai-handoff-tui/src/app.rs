//! The ratatui application: state, key handling, and drawing for the
//! Overview / Capsule / Account / Settings tabs.
//!
//! `on_key` is kept independent of the terminal (it only mutates state and, on
//! a Settings save, writes config) so the interaction logic is unit-testable
//! without a TTY. The draw + event loop are the thin, untested shell.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
use ai_handoff_core::dashboard::{CheckStatus, DashboardSnapshot};

use crate::capsule_ops;
use crate::edit::{self, EditAction};
use crate::viewmodel::{
    capsule_tree, health_rows, settings_rows, CapsuleAgent, HealthRow, SettingRow, UsageView,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Capsule,
    Account,
    Settings,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Overview, Tab::Capsule, Tab::Account, Tab::Settings];
    /// Translation key for the tab's title (resolved at render time via `t!`).
    fn title_key(self) -> &'static str {
        match self {
            Tab::Overview => "tab.overview",
            Tab::Capsule => "tab.capsule",
            Tab::Account => "tab.account",
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
        _ => return String::new(),
    };
    t!(desc_key).into_owned()
}

/// The description for `key`, or the generic Settings hint when there is none.
fn setting_desc_or_hint(key: Option<&str>) -> String {
    let desc = key.map(setting_desc).unwrap_or_default();
    if desc.is_empty() {
        t!("hint.settings").into_owned()
    } else {
        desc
    }
}

/// The quit-confirmation hint, in the active language.
fn quit_hint() -> String {
    t!("hint.quit").into_owned()
}

/// Claude = orange, Codex = purple (the token-split donut + legend).
const CLAUDE_COLOR: Color = Color::Rgb(230, 140, 30);
const CODEX_COLOR: Color = Color::Rgb(150, 90, 220);
/// A lighter purple for the "Codex" label text in the Capsule / Account trees.
const CODEX_LABEL_COLOR: Color = Color::Rgb(185, 150, 235);

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

/// The reset-credit ("초기화권") fetch is an explicit, network-gated action.
#[derive(Clone, PartialEq, Eq)]
enum CreditsState {
    /// Not fetched yet (the count needs an authenticated backend call).
    Idle,
    Loaded(i64),
    Error(String),
}

/// One agent's account picture: who is signed in, their live limits, and the
/// saved snapshots in the pool.
#[derive(Default)]
struct AgentAccount {
    identity: Option<account::Identity>,
    status: Option<account::AccountStatus>,
    slots: Vec<account::PoolSlot>,
}

/// Both agents' account data for the Account tab.
#[derive(Default)]
struct AccountData {
    codex: AgentAccount,
    claude: AgentAccount,
}

impl AccountData {
    /// Scan the live system (rollout limits, auth identity, pool snapshots).
    fn load_live() -> Self {
        AccountData {
            codex: AgentAccount {
                identity: account::codex_identity(),
                status: account::codex_status(),
                slots: account::list_slots(Agent::Codex),
            },
            claude: AgentAccount {
                identity: account::claude_identity(),
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
    config_path: PathBuf,
    // --- Account tab state ---
    account: AccountData,
    /// Whether focus is on the account tree or the detail pane.
    acc_focus: AccFocus,
    /// Selected row in the flattened account tree.
    acc_sel: usize,
    /// A delete needs a second confirm press; armed here.
    acc_confirm_delete: bool,
    /// The (Codex) reset-credit count: fetched on demand over the network.
    acc_credits: CreditsState,
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

impl App {
    /// Build the app by scanning the live system (logs, config, health).
    pub fn load() -> Self {
        let snapshot = ai_handoff_core::dashboard::dashboard_snapshot();
        let usage = UsageView::from_events(&ai_handoff_usage::scan_default());
        let cfg = ai_handoff_core::config::load();
        let config_path = ai_handoff_core::paths::config_path();
        let mut app = App::new(snapshot, usage, settings_rows(&cfg), config_path);
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
        App {
            tab: Tab::Overview,
            focus_content: false,
            snapshot,
            usage,
            settings,
            settings_idx: 0,
            config_path,
            account: AccountData::default(),
            acc_focus: AccFocus::Tree,
            acc_sel: 0,
            acc_confirm_delete: false,
            acc_credits: CreditsState::Idle,
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

    /// The event loop. Returns when the user quits.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.should_quit {
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.on_key(key);
                    }
                }
            }
        }
        Ok(())
    }

    /// Handle one keypress. Pure except for a config file write on Settings edit.
    pub fn on_key(&mut self, key: KeyEvent) {
        // While editing a capsule goal, the editor owns every key (so typing
        // 'q', a digit, or Tab inserts text instead of navigating).
        if self.tab == Tab::Capsule && self.cap_focus == CapFocus::Editing {
            self.cap_editing_key(key);
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
            KeyCode::Char('3') => return self.goto(Tab::Account),
            KeyCode::Char('4') => return self.goto(Tab::Settings),
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
            Tab::Account => self.on_account_key(key),
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
            Tab::Account => {
                self.acc_focus = AccFocus::Tree;
                self.acc_confirm_delete = false;
                t!("hint.account_tree").into_owned()
            }
            Tab::Settings => setting_desc_or_hint(self.settings.first().map(|r| r.key)),
        };
    }

    /// q/Esc: inside a tab's content, just leave it (back to the tab bar). On a
    /// top tab, arm a quit confirmation; a second q/Esc actually quits.
    fn on_back(&mut self) {
        if self.focus_content {
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

    fn on_settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.settings_idx > 0 {
                    self.settings_idx -= 1;
                }
                self.show_setting_desc();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.settings_idx + 1 < self.settings.len() {
                    self.settings_idx += 1;
                }
                self.show_setting_desc();
            }
            KeyCode::Char(' ') => self.edit_current(EditAction::Toggle),
            KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('=') => {
                self.edit_current(EditAction::Next)
            }
            KeyCode::Left | KeyCode::Char('-') => self.edit_current(EditAction::Prev),
            _ => {}
        }
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
        let Some(raw) = edit::next_raw(row.kind, &row.value, action) else {
            self.status = t!("status.cannot_edit", key = row.key).into_owned();
            return;
        };
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
                Err(e) => {
                    self.status = t!("status.autostart_failed", err = e).into_owned()
                }
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
                let parsed = serde_json::from_str(&raw).ok();
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
                let label = if slot.active {
                    format!("• {}  [{}]", slot.label, t!("account.active"))
                } else {
                    format!("• {}", slot.label)
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
            KeyCode::Char('+') => {
                let agent = self.acc_selected_agent();
                self.acc_add(agent);
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ') => {
                match rows.get(self.acc_sel).map(|r| r.target) {
                    Some(AccTarget::Add(agent)) => self.acc_add(agent),
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
            KeyCode::Char('r') => self.acc_refresh_credits(),
            _ => {}
        }
    }

    /// Add an account to the pool. Interim behavior: snapshot the agent's
    /// current live auth into a new slot (labelled by the detected email). The
    /// planned OAuth login flow is documented in docs/account-add-oauth-design.md.
    fn acc_add(&mut self, agent: Agent) {
        match account::snapshot_current(agent) {
            Ok(label) => {
                self.reload_account();
                self.status = t!("status.account_captured", label = label).into_owned();
            }
            Err(e) => {
                self.status = t!("status.account_capture_failed", err = e.to_string()).into_owned()
            }
        }
    }

    /// Make the selected saved account the live one (file swap).
    fn acc_switch(&mut self) {
        let Some((agent, i)) = self.acc_selected_slot() else {
            return;
        };
        let label = self.account.agent(agent).slots[i].label.clone();
        match account::switch_slot(agent, &label) {
            Ok(()) => {
                // The live account changed — the cached credit count is stale.
                self.acc_credits = CreditsState::Idle;
                self.reload_account();
                self.status = t!("status.account_switched", label = label).into_owned();
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
        let label = self.account.agent(agent).slots[i].label.clone();
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

    /// Fetch the Codex reset-credit count (the one network call; uses the token
    /// only to set the auth header — never logged or displayed).
    fn acc_refresh_credits(&mut self) {
        if self.acc_selected_agent() != Agent::Codex {
            return;
        }
        match crate::account_api::fetch_reset_credits() {
            Ok(n) => {
                self.acc_credits = CreditsState::Loaded(n);
                self.status = t!("status.account_credits_ok", count = n).into_owned();
            }
            Err(e) => {
                self.status = t!("status.account_credits_failed", err = e.clone()).into_owned();
                self.acc_credits = CreditsState::Error(e);
            }
        }
    }

    /// Re-scan the live system after a pool change and clamp the selection.
    fn reload_account(&mut self) {
        self.account = AccountData::load_live();
        let n = self.acc_rows().len();
        if self.acc_sel >= n {
            self.acc_sel = n.saturating_sub(1);
        }
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
            Tab::Account => self.draw_account(f, chunks[1]),
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
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(tabs, area);
    }

    fn draw_overview(&self, f: &mut Frame, area: Rect) {
        // Token usage sits on the left (donut + legend); health on the right.
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area);

        // Left column: a donut split (top) over a colored legend (bottom).
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(7), Constraint::Length(7)])
            .split(cols[0]);
        self.draw_token_donut(f, left[0]);
        self.draw_token_legend(f, left[1]);

        let rows = health_rows(&self.snapshot).into_iter().map(health_table_row);
        let table = Table::new(rows, [Constraint::Length(18), Constraint::Length(8), Constraint::Min(10)])
            .header(
                Row::new([
                    t!("table.check").into_owned(),
                    t!("table.status").into_owned(),
                    t!("table.detail").into_owned(),
                ])
                .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(t!("overview.health").into_owned()),
            );
        f.render_widget(table, cols[1]);
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
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else {
                        Style::default().add_modifier(Modifier::REVERSED)
                    };
                    Line::from(Span::styled(text, style))
                } else if let CapTarget::Agent(ai) = r.target {
                    // Brand-color the agent rows (Codex = purple, ClaudeCode = orange).
                    match agent_label_color(&self.cap_tree[ai].agent) {
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
        let list = Paragraph::new(lines).block(focus_block(t!("capsule.list_title"), tree_focused));
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
            .map(|c| match c.consumption.state {
                ai_handoff_core::capsule::ConsumptionState::Pending => state_label("pending"),
                ai_handoff_core::capsule::ConsumptionState::Consumed => state_label("consumed"),
            })
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
                action_style(focused),
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
                action_style(focused),
            ));
        }
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(" {} ", t!("capsule.btn_edit")),
            action_style(focused),
        ));
        let bar = Paragraph::new(Line::from(spans)).block(focus_block(
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
            let banner = t!("capsule.edit_banner", field = field_label(field), hint = hint);
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
                .block(focus_block(t!("capsule.edit_title"), true));
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
            .block(focus_block(title, detail_active));
        f.render_widget(body, area);
    }

    /// Lines for the capsule body: editable fields (the selected one highlighted
    /// when the detail pane is active) followed by read-only context.
    fn capsule_body_lines(&self, detail: &CapDetail) -> Vec<Line<'static>> {
        let Some(c) = detail.parsed.as_ref() else {
            return detail.raw.lines().map(|l| Line::from(l.to_string())).collect();
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
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else if i == self.cap_field {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(text, style)));
        }

        let state = match c.consumption.state {
            ai_handoff_core::capsule::ConsumptionState::Pending => state_label("pending"),
            ai_handoff_core::capsule::ConsumptionState::Consumed => state_label("consumed"),
        };
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
        lines.push(Line::from(format!("  {}: {state}", t!("capsule.field_state"))));
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

    fn draw_token_donut(&self, f: &mut Frame, area: Rect) {
        let (claude, codex) = self.source_split();
        let total = claude + codex;
        let block = Block::default()
            .borders(Borders::ALL)
            .title(t!("overview.token_split").into_owned());
        if total == 0 {
            f.render_widget(
                Paragraph::new(t!("overview.no_usage").into_owned()).block(block),
                area,
            );
            return;
        }
        let claude_frac = claude as f64 / total as f64;
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
                ctx.draw(&Points { coords: &claude_pts, color: CLAUDE_COLOR });
                ctx.draw(&Points { coords: &codex_pts, color: CODEX_COLOR });
            });
        f.render_widget(canvas, area);
    }

    fn draw_token_legend(&self, f: &mut Frame, area: Rect) {
        let (claude, codex) = self.source_split();
        let total = claude + codex;
        let pct = |n: u64| if total > 0 { n as f64 / total as f64 * 100.0 } else { 0.0 };
        let total_line = t!(
            "overview.total",
            tokens = human_tokens(total),
            cost = format!("{:.2}", self.usage.total.cost_usd)
        );
        let lines = vec![
            Line::from(total_line.into_owned()),
            Line::from(vec![
                Span::styled("● claude  ", Style::default().fg(CLAUDE_COLOR)),
                Span::raw(format!("{:>7}  {:>4.0}%", human_tokens(claude), pct(claude))),
            ]),
            Line::from(vec![
                Span::styled("● codex   ", Style::default().fg(CODEX_COLOR)),
                Span::raw(format!("{:>7}  {:>4.0}%", human_tokens(codex), pct(codex))),
            ]),
            Line::from(t!("overview.estimate").into_owned()).italic(),
        ];
        f.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
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
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else {
                        Style::default().add_modifier(Modifier::REVERSED)
                    };
                    Line::from(Span::styled(text, style))
                } else if let AccTarget::Header(agent) = r.target {
                    Line::from(Span::styled(
                        text,
                        Style::default().fg(agent_color(agent)).add_modifier(Modifier::BOLD),
                    ))
                } else {
                    Line::from(text)
                }
            })
            .collect();
        let list = Paragraph::new(lines).block(focus_block(t!("account.list_title"), tree_focused));
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
                action_style(focused && has_slot),
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
                action_style(focused && has_slot),
            ));
        }
        let bar = Paragraph::new(Line::from(spans))
            .block(focus_block(t!("account.actions"), focused));
        f.render_widget(bar, area);
    }

    /// The status pane: signed-in identity, plan, the two rate-limit gauges,
    /// and (Codex only) the reset-credit count.
    fn draw_account_status(&self, f: &mut Frame, area: Rect, focused: bool) {
        let agent = self.acc_selected_agent();
        let data = self.account.agent(agent);

        let block = focus_block(agent_name(agent).to_string(), focused);
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

        // Header: signed-in email + plan.
        let signed = match data.identity.as_ref().and_then(|i| i.email.as_deref()) {
            Some(email) => t!("account.signed_in", email = email).into_owned(),
            None => t!("account.not_signed_in").into_owned(),
        };
        let header = Paragraph::new(vec![
            Line::from(signed),
            Line::from(t!("account.plan", plan = self.account_plan(agent)).into_owned()),
        ]);
        f.render_widget(header, sections[0]);

        // The two windows (5-hour, weekly) as gauges.
        let status = data.status.as_ref();
        self.draw_window(
            f,
            sections[1],
            t!("account.five_hour").into_owned(),
            status.and_then(|s| s.five_hour.as_ref()),
        );
        self.draw_window(
            f,
            sections[2],
            t!("account.weekly").into_owned(),
            status.and_then(|s| s.weekly.as_ref()),
        );

        // Notes: reset credits (Codex) + capture time / no-data.
        let mut lines: Vec<Line> = Vec::new();
        if status.is_none() {
            lines.push(Line::from(t!("account.no_data").into_owned()).fg(Color::DarkGray));
        }
        if agent == Agent::Codex {
            lines.push(Line::from(Span::styled(
                t!("account.reset_credits").into_owned(),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(self.credits_line());
            lines.push(Line::from(t!("account.reset_credits_hint").into_owned()).fg(Color::DarkGray));
        }
        if let Some(ms) = status.and_then(|s| s.captured_at) {
            lines.push(Line::from(fmt_captured(ms)).fg(Color::DarkGray));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), sections[3]);
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

    /// The reset-credit line, reflecting the (network-gated) fetch state.
    fn credits_line(&self) -> Line<'static> {
        match &self.acc_credits {
            CreditsState::Loaded(n) => {
                Line::from(t!("account.reset_credits_value", count = *n).into_owned())
            }
            CreditsState::Error(e) => {
                Line::from(t!("account.reset_credits_error", err = e.clone()).into_owned())
                    .fg(Color::Red)
            }
            CreditsState::Idle => {
                Line::from(t!("account.reset_credits_press").into_owned()).fg(Color::DarkGray)
            }
        }
    }

    /// Resolve the plan label (rollout plan_type, then JWT plan, then "unknown").
    fn account_plan(&self, agent: Agent) -> String {
        let data = self.account.agent(agent);
        data.status
            .as_ref()
            .and_then(|s| s.plan_type.clone())
            .or_else(|| data.identity.as_ref().and_then(|i| i.plan_type.clone()))
            .unwrap_or_else(|| t!("account.plan_unknown").into_owned())
    }

    fn draw_settings(&self, f: &mut Frame, area: Rect) {
        let rows = self.settings.iter().enumerate().map(|(i, r)| {
            let style = if i == self.settings_idx {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default()
            };
            Row::new([Cell::from(r.key), Cell::from(r.value.clone())]).style(style)
        });
        let table = Table::new(rows, [Constraint::Min(36), Constraint::Length(14)])
            .header(
                Row::new([t!("table.setting").into_owned(), t!("table.value").into_owned()])
                    .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(t!("settings.title").into_owned()),
            );
        f.render_widget(table, area);
    }

    fn draw_status(&self, f: &mut Frame, area: Rect) {
        let para = Paragraph::new(Span::raw(&self.status))
            .block(Block::default().borders(Borders::ALL));
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

/// Brand color for an agent (Codex = light purple, Claude = orange).
fn agent_color(agent: Agent) -> Color {
    match agent {
        Agent::Codex => CODEX_LABEL_COLOR,
        Agent::Claude => CLAUDE_COLOR,
    }
}

/// Brand color for a capsule-tree agent string ("Codex" / "ClaudeCode"), or
/// `None` for an unrecognised agent.
fn agent_label_color(name: &str) -> Option<Color> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("codex") {
        Some(CODEX_LABEL_COLOR)
    } else if lower.contains("claude") {
        Some(CLAUDE_COLOR)
    } else {
        None
    }
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

/// Local "as of MM-DD HH:MM" stamp for a unix-millis capture time.
fn fmt_captured(ms: i64) -> String {
    let dt = chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default();
    format!("· {}", dt.with_timezone(&chrono::Local).format("%m-%d %H:%M"))
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
        Err(std::io::Error::other(format!("autostart command exited with {status}")))
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

/// Translated consumption-state word ("pending" / "consumed").
fn state_label(state: &str) -> String {
    match state {
        "pending" => t!("state.pending").into_owned(),
        "consumed" => t!("state.consumed").into_owned(),
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
fn focus_block(title: impl Into<String>, focused: bool) -> Block<'static> {
    let (border_type, style) = if focused {
        (BorderType::Thick, Style::default().fg(Color::Yellow))
    } else {
        (BorderType::Plain, Style::default().fg(Color::DarkGray))
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(style)
        .title(title.into())
}

/// Style for an action-bar button: emphasised when its pane has focus.
fn action_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    }
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

/// One-line capsule label: date + state + a short summary preview.
fn capsule_label(cap: &ai_handoff_core::dashboard::CapsuleSummary) -> String {
    let when = cap.created_at.get(..10).unwrap_or(&cap.created_at);
    let preview = if cap.summary_preview.is_empty() {
        "(no summary)"
    } else {
        cap.summary_preview.as_str()
    };
    format!("{when} [{}] {}", cap.state, truncate(preview, 30))
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

fn status_style(status: &CheckStatus) -> (&'static str, Color) {
    match status {
        CheckStatus::Ok => ("ok", Color::Green),
        CheckStatus::Warning => ("warn", Color::Yellow),
        CheckStatus::Error => ("error", Color::Red),
        CheckStatus::Missing => ("missing", Color::DarkGray),
        CheckStatus::Unknown => ("?", Color::DarkGray),
    }
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
        app.on_key(key(KeyCode::Char('4'))); // Capsule -> Settings
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
        app.on_key(key(KeyCode::Char('4'))); // Settings tab bar
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

    /// Build an app with an injected Codex account (one slot) for nav tests.
    fn account_app() -> App {
        let mut app = test_app();
        app.account.codex = AgentAccount {
            identity: Some(account::Identity {
                email: Some("dev@example.com".into()),
                account_id: Some("acc-1".into()),
                plan_type: Some("pro".into()),
            }),
            status: Some(account::AccountStatus {
                plan_type: Some("team".into()),
                five_hour: Some(account::RateWindow {
                    used_percent: 18.0,
                    window_minutes: 300,
                    resets_at: Some(chrono::Utc::now().timestamp() + 7_680),
                }),
                weekly: Some(account::RateWindow {
                    used_percent: 61.0,
                    window_minutes: 10080,
                    resets_at: None,
                }),
                captured_at: Some(chrono::Utc::now().timestamp_millis()),
            }),
            slots: vec![
                account::PoolSlot {
                    label: "dev@example.com".into(),
                    path: std::path::PathBuf::from("/p/dev.authsnap"),
                    active: true,
                },
                account::PoolSlot {
                    label: "alt@example.com".into(),
                    path: std::path::PathBuf::from("/p/alt.authsnap"),
                    active: false,
                },
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
    fn account_plan_prefers_rollout_then_jwt() {
        let app = account_app();
        // status.plan_type ("team") wins over identity.plan_type ("pro").
        assert_eq!(app.account_plan(Agent::Codex), "team");
        // Claude has neither here -> the translated "unknown".
        assert_eq!(app.account_plan(Agent::Claude), t!("account.plan_unknown").into_owned());
    }

    #[test]
    fn account_nav_enters_detail_on_a_slot_and_back() {
        let mut app = account_app();
        app.on_key(key(KeyCode::Char('3'))); // -> Account tab bar
        assert_eq!(app.tab, Tab::Account);
        assert!(!app.focus_content);
        app.on_key(key(KeyCode::Down)); // descend into the tree
        assert!(app.focus_content);
        assert_eq!(app.acc_focus, AccFocus::Tree);
        app.on_key(key(KeyCode::Down)); // move onto the first Codex slot
        assert!(matches!(app.acc_target(), Some(AccTarget::Slot(Agent::Codex, 0))));
        app.on_key(key(KeyCode::Enter)); // cross into the detail pane
        assert_eq!(app.acc_focus, AccFocus::Detail);
        app.on_key(key(KeyCode::Left)); // back to the tree
        assert_eq!(app.acc_focus, AccFocus::Tree);
    }

    #[test]
    fn window_line_interpolates_in_all_locales() {
        for loc in ["en", "ko", "ja", "zh"] {
            rust_i18n::set_locale(loc);
            let s = t!(
                "account.window_line",
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
        rust_i18n::set_locale("en");
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
                tokens: Tokens { input: 10, ..Default::default() },
            },
            UsageEvent {
                source: Source::Codex,
                project: "p".into(),
                session: "s".into(),
                model: "gpt-5.5".into(),
                day: "2026-06-17".into(),
                tokens: Tokens { input: 4, ..Default::default() },
            },
        ];
        let dir = tempfile::tempdir().unwrap();
        let snapshot =
            ai_handoff_core::dashboard::dashboard_snapshot_for(dir.path(), dir.path());
        let app = App::new(
            snapshot,
            UsageView::from_events(&events),
            vec![],
            dir.path().join("config.toml"),
        );
        assert_eq!(app.source_split(), (10, 4));
    }

    #[test]
    fn tab_titles_translate_with_locale() {
        rust_i18n::set_locale("ko");
        assert_eq!(t!("tab.overview"), "개요");
        rust_i18n::set_locale("ja");
        assert_eq!(t!("tab.account"), "アカウント");
        rust_i18n::set_locale("zh");
        assert_eq!(t!("tab.settings"), "设置");
        rust_i18n::set_locale("en");
        assert_eq!(t!("tab.capsule"), "Capsule");
    }

    #[test]
    fn tab_cycles_forward_and_number_keys_jump() {
        let mut app = test_app();
        assert_eq!(app.tab, Tab::Overview);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.tab, Tab::Capsule);
        app.on_key(key(KeyCode::Char('4')));
        assert_eq!(app.tab, Tab::Settings);
        app.on_key(key(KeyCode::Char('1')));
        assert_eq!(app.tab, Tab::Overview);
    }

    #[test]
    fn entering_a_tab_does_not_auto_activate_its_content() {
        let mut app = test_app();
        app.on_key(key(KeyCode::Char('4'))); // -> Settings, on the tab bar
        assert_eq!(app.tab, Tab::Settings);
        assert!(!app.focus_content, "Settings must not auto-enter edit mode");
        // Space at the tab-bar level descends but must not edit the first row.
        app.on_key(key(KeyCode::Char(' ')));
        assert!(app.focus_content);
        assert_eq!(app.settings[0].value, "true", "descend must not toggle");
        // Now a second Space actually edits.
        app.on_key(key(KeyCode::Char(' ')));
        assert_eq!(app.settings[0].value, "false");
    }

    #[test]
    fn down_enters_content_without_moving_selection() {
        let mut app = test_app();
        app.on_key(key(KeyCode::Char('4'))); // -> Settings tab bar
        app.on_key(key(KeyCode::Down)); // descend
        assert!(app.focus_content);
        assert_eq!(app.settings_idx, 0, "the descending Down must not also move");
    }

    #[test]
    fn settings_navigation_and_edit_persist() {
        let mut app = test_app();
        app.tab = Tab::Settings;
        app.focus_content = true;
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
        rust_i18n::set_locale("en");
        let mut app = test_app();
        app.on_key(key(KeyCode::Char('4'))); // -> Settings tab bar
        app.on_key(key(KeyCode::Down)); // descend; status = desc of row 0
        assert!(app.focus_content);
        assert_eq!(app.settings_idx, 0);
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
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.settings_idx, 1);
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.settings_idx, 0);
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
            redaction: RedactionMeta { applied: true, ruleset: "default-v2".into() },
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

        // Toggle state: disk + in-memory both become consumed.
        app.on_key(key(KeyCode::Char('s')));
        assert_eq!(app.cap_tree[0].projects[0].capsules[0].state, "consumed");
        let on_disk: Capsule =
            serde_json::from_slice(&std::fs::read(&cap_path).unwrap()).unwrap();
        assert_eq!(on_disk.consumption.state, ConsumptionState::Consumed);

        // Edit the goal: 'e' loads it, type, Enter saves.
        app.on_key(key(KeyCode::Char('e')));
        assert_eq!(app.cap_focus, CapFocus::Editing);
        assert_eq!(app.cap_edit_buf, "old goal");
        app.on_key(key(KeyCode::Char('!')));
        app.on_key(key(KeyCode::Enter));
        let on_disk: Capsule =
            serde_json::from_slice(&std::fs::read(&cap_path).unwrap()).unwrap();
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
