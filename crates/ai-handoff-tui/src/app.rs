//! The ratatui application: state, key handling, and drawing for the
//! Overview / Usage / Settings tabs.
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
        BarChart, Block, BorderType, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap,
    },
    DefaultTerminal, Frame,
};

use ai_handoff_core::dashboard::{CheckStatus, DashboardSnapshot};
use ai_handoff_usage::{aggregate::Group, Dimension};

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
    Settings,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Overview, Tab::Capsule, Tab::Usage, Tab::Settings];
    fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Capsule => "Capsule",
            Tab::Usage => "Usage",
            Tab::Settings => "Settings",
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

const DIMS: [Dimension; 4] = [
    Dimension::Day,
    Dimension::Model,
    Dimension::Project,
    Dimension::Source,
];

fn dim_name(dim: Dimension) -> &'static str {
    match dim {
        Dimension::Day => "day",
        Dimension::Model => "model",
        Dimension::Project => "project",
        Dimension::Source => "source",
    }
}

const DEFAULT_HINT: &str =
    "q/Esc back · Tab/1-4 or ←/→ switch tab · ↓/Space/Enter open tab";
const QUIT_HINT: &str = "한 번 더 q/Esc 누르면 완전히 종료됩니다 (press q/Esc again to quit)";

/// Claude = orange, Codex = purple (the token-split donut + legend).
const CLAUDE_COLOR: Color = Color::Rgb(230, 140, 30);
const CODEX_COLOR: Color = Color::Rgb(150, 90, 220);

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

pub struct App {
    pub tab: Tab,
    /// Whether the focus is inside the current tab's content (vs. the tab bar).
    /// A top tab only descends into its content on ↓/Space/Enter.
    focus_content: bool,
    snapshot: DashboardSnapshot,
    usage: UsageView,
    usage_dim: Dimension,
    settings: Vec<SettingRow>,
    settings_idx: usize,
    config_path: PathBuf,
    // --- Capsule tab state ---
    cap_tree: Vec<CapsuleAgent>,
    cap_expanded_agents: HashSet<usize>,
    cap_expanded_projects: HashSet<(usize, usize)>,
    cap_sel: usize,
    /// Whether focus is on the tree, the detail pane, or the goal editor.
    cap_focus: CapFocus,
    /// Vertical scroll offset of the detail body.
    cap_scroll: u16,
    /// A delete needs a second confirm press; armed here.
    cap_confirm_delete: bool,
    /// The working buffer while editing a capsule's goal.
    cap_edit_buf: String,
    /// Cached parsed/raw detail of the currently-selected capsule.
    cap_detail: Option<CapDetail>,
    status: String,
    should_quit: bool,
    /// Armed at the root (empty history) by a first q/Esc; a second one quits.
    confirm_quit: bool,
    /// Tab visit history; q/Esc pops it ("back") before arming a quit.
    nav_stack: Vec<Tab>,
}

impl App {
    /// Build the app by scanning the live system (logs, config, health).
    pub fn load() -> Self {
        let snapshot = ai_handoff_core::dashboard::dashboard_snapshot();
        let usage = UsageView::from_events(&ai_handoff_usage::scan_default());
        let cfg = ai_handoff_core::config::load();
        let config_path = ai_handoff_core::paths::config_path();
        App::new(snapshot, usage, settings_rows(&cfg), config_path)
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
            usage_dim: Dimension::Day,
            settings,
            settings_idx: 0,
            config_path,
            cap_tree,
            cap_expanded_agents,
            cap_expanded_projects: HashSet::new(),
            cap_sel: 0,
            cap_focus: CapFocus::Tree,
            cap_scroll: 0,
            cap_confirm_delete: false,
            cap_edit_buf: String::new(),
            cap_detail: None,
            status: DEFAULT_HINT.to_string(),
            should_quit: false,
            confirm_quit: false,
            nav_stack: Vec::new(),
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
            self.status = DEFAULT_HINT.to_string();
        }
        // Tab / Shift-Tab / number keys switch tabs from either level and land
        // back on the tab bar (so each tab is re-entered explicitly).
        match key.code {
            KeyCode::Tab => return self.goto(self.tab.next()),
            KeyCode::BackTab => return self.goto(self.tab.prev()),
            KeyCode::Char('1') => return self.goto(Tab::Overview),
            KeyCode::Char('2') => return self.goto(Tab::Capsule),
            KeyCode::Char('3') => return self.goto(Tab::Usage),
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
            Tab::Usage => self.on_usage_key(key),
            Tab::Settings => self.on_settings_key(key),
        }
    }

    /// Switch tabs, recording where we came from so q/Esc can step back. Always
    /// lands on the tab bar (content focus is dropped).
    fn goto(&mut self, target: Tab) {
        if target != self.tab {
            self.nav_stack.push(self.tab);
            self.tab = target;
        }
        self.focus_content = false;
        self.confirm_quit = false;
        self.status = DEFAULT_HINT.to_string();
    }

    /// Descend from the tab bar into the current tab's content.
    fn enter_content(&mut self) {
        self.focus_content = true;
        self.status = match self.tab {
            Tab::Overview => DEFAULT_HINT.to_string(),
            Tab::Capsule => {
                self.cap_focus = CapFocus::Tree;
                self.cap_confirm_delete = false;
                self.cap_load_content();
                "↑/↓ move · Enter/→ expand or open capsule · ← collapse · q/Esc back".to_string()
            }
            Tab::Usage => "g cycle breakdown · q/Esc back".to_string(),
            Tab::Settings => {
                "↑/↓ select · space toggle · ←/→ change · q/Esc back".to_string()
            }
        };
    }

    /// q/Esc: leave content focus first, then step back through visited tabs;
    /// at the root (empty history) arm a quit, and quit on the second press.
    fn on_back(&mut self) {
        if self.focus_content {
            self.focus_content = false;
            self.confirm_quit = false;
            self.status = DEFAULT_HINT.to_string();
            return;
        }
        if let Some(prev) = self.nav_stack.pop() {
            self.tab = prev;
            self.confirm_quit = false;
            self.status = DEFAULT_HINT.to_string();
        } else if self.confirm_quit {
            self.should_quit = true;
        } else {
            self.confirm_quit = true;
            self.status = QUIT_HINT.to_string();
        }
    }

    fn on_usage_key(&mut self, key: KeyEvent) {
        if let KeyCode::Char('g') = key.code {
            let idx = DIMS.iter().position(|d| *d == self.usage_dim).unwrap_or(0);
            self.usage_dim = DIMS[(idx + 1) % DIMS.len()];
        }
    }

    fn on_settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.settings_idx > 0 {
                    self.settings_idx -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.settings_idx + 1 < self.settings.len() {
                    self.settings_idx += 1;
                }
            }
            KeyCode::Char(' ') => self.edit_current(EditAction::Toggle),
            KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('=') => {
                self.edit_current(EditAction::Next)
            }
            KeyCode::Left | KeyCode::Char('-') => self.edit_current(EditAction::Prev),
            _ => {}
        }
    }

    /// Apply an edit to the selected setting and persist it.
    fn edit_current(&mut self, action: EditAction) {
        let Some(row) = self.settings.get(self.settings_idx).cloned() else {
            return;
        };
        let Some(raw) = edit::next_raw(row.kind, &row.value, action) else {
            self.status = format!("{}: cannot edit current value", row.key);
            return;
        };
        match edit::commit(&self.config_path, row.key, &raw) {
            Ok(_) => {
                self.settings[self.settings_idx].value = raw.clone();
                self.status = format!("saved {} = {}", row.key, raw);
            }
            Err(e) => self.status = format!("{}: {e}", row.key),
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
        self.cap_scroll = 0;
        self.cap_confirm_delete = false;
        self.cap_load_content();
        self.status =
            "s toggle state · d delete · e/Enter edit goal · ↑/↓ scroll · ← back to list"
                .to_string();
    }

    /// Keys while the right detail pane has focus (action bar + body).
    fn cap_detail_key(&mut self, key: KeyEvent) {
        // Any key other than a second 'd'/'y' cancels an armed delete.
        let confirming = self.cap_confirm_delete;
        if confirming && !matches!(key.code, KeyCode::Char('d') | KeyCode::Char('y')) {
            self.cap_confirm_delete = false;
            self.status = "delete cancelled".to_string();
        }
        match key.code {
            KeyCode::Left => self.cap_focus_tree(),
            KeyCode::Up | KeyCode::Char('k') => self.cap_scroll = self.cap_scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.cap_scroll = self.cap_scroll.saturating_add(1),
            KeyCode::Char('s') => self.cap_toggle_state(),
            KeyCode::Char('d') | KeyCode::Char('y') if confirming => self.cap_delete(),
            KeyCode::Char('d') => {
                self.cap_confirm_delete = true;
                self.status = "delete this capsule? press d or y to confirm, any other key to cancel"
                    .to_string();
            }
            KeyCode::Char('e') | KeyCode::Enter => self.cap_begin_edit(),
            _ => {}
        }
    }

    /// Return focus to the tree (the 3/10 side).
    fn cap_focus_tree(&mut self) {
        self.cap_focus = CapFocus::Tree;
        self.cap_confirm_delete = false;
        self.status =
            "↑/↓ move · Enter/→ expand or open capsule · ← collapse · q/Esc back".to_string();
    }

    /// Keys while editing the selected capsule's goal.
    fn cap_editing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.cap_commit_edit(),
            KeyCode::Esc => {
                self.cap_focus = CapFocus::Detail;
                self.status = "edit cancelled".to_string();
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
                self.status = format!("state → {new_state}");
            }
            Err(e) => self.status = format!("state change failed: {e}"),
        }
    }

    /// Begin editing the selected capsule's goal (loads the current goal).
    fn cap_begin_edit(&mut self) {
        let goal = self
            .cap_detail
            .as_ref()
            .and_then(|d| d.parsed.as_ref())
            .map(|c| c.summary.goal.clone());
        match goal {
            Some(goal) => {
                self.cap_edit_buf = goal;
                self.cap_focus = CapFocus::Editing;
                self.status = "editing goal — Enter save · Esc cancel".to_string();
            }
            None => self.status = "this capsule cannot be edited (not a valid capsule)".to_string(),
        }
    }

    /// Save the edited goal to disk and the in-memory tree.
    fn cap_commit_edit(&mut self) {
        let Some((ai, pi, ci)) = self.selected_capsule() else {
            self.cap_focus = CapFocus::Detail;
            return;
        };
        let path = self.cap_tree[ai].projects[pi].capsules[ci].path.clone();
        let goal = self.cap_edit_buf.clone();
        match capsule_ops::set_goal(Path::new(&path), &goal) {
            Ok(()) => {
                self.cap_tree[ai].projects[pi].capsules[ci].summary_preview = goal;
                self.cap_detail = None;
                self.cap_load_content();
                self.status = "goal saved".to_string();
            }
            Err(e) => self.status = format!("save failed: {e}"),
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
            self.status = format!("delete failed: {e}");
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
        self.status = "capsule deleted".to_string();
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
                self.cap_scroll = 0;
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
            Tab::Settings => self.draw_settings(f, chunks[1]),
        }
        self.draw_status(f, chunks[2]);
    }

    fn draw_tabs(&self, f: &mut Frame, area: Rect) {
        let titles = Tab::ALL.iter().map(|t| Line::from(t.title()));
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
            .header(Row::new(["Check", "Status", "Detail"]).style(Style::default().add_modifier(Modifier::BOLD)))
            .block(Block::default().borders(Borders::ALL).title("Health"));
        f.render_widget(table, cols[1]);
    }

    fn draw_capsule(&self, f: &mut Frame, area: Rect) {
        let rows = self.cap_rows();
        if rows.is_empty() {
            let para = Paragraph::new(
                "No capsules captured yet.\n\nCapsules appear here once /handoff hands off context \
                 between Codex and Claude. Each is grouped under its agent and project.",
            )
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Capsule"));
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
                } else {
                    Line::from(text)
                }
            })
            .collect();
        let list = Paragraph::new(lines)
            .block(focus_block("Capsules (↓/Space/Enter to open)", tree_focused));
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
                ai_handoff_core::capsule::ConsumptionState::Pending => "pending",
                ai_handoff_core::capsule::ConsumptionState::Consumed => "consumed",
            });
        let mut spans = vec![
            Span::raw(" "),
            Span::styled(
                format!(" state: {} ", state.unwrap_or("—")),
                Style::default().fg(Color::Black).bg(Color::Gray),
            ),
            Span::raw("  "),
            Span::styled(" [s] toggle state ", action_style(focused)),
            Span::raw("  "),
        ];
        if self.cap_confirm_delete {
            spans.push(Span::styled(
                " [d] confirm delete ",
                Style::default().fg(Color::White).bg(Color::Red),
            ));
        } else {
            spans.push(Span::styled(" [d] delete ", action_style(focused)));
        }
        spans.push(Span::raw("  "));
        spans.push(Span::styled(" [e/Enter] edit goal ", action_style(focused)));
        let bar = Paragraph::new(Line::from(spans))
            .block(focus_block("Actions", focused && self.cap_focus == CapFocus::Detail));
        f.render_widget(bar, area);
    }

    /// The capsule body: a readable view (or the goal editor when editing).
    fn draw_capsule_body(&self, f: &mut Frame, area: Rect, focused: bool) {
        if self.cap_focus == CapFocus::Editing {
            let lines = vec![
                Line::from("Editing goal (Enter to save · Esc to cancel):").italic(),
                Line::from(""),
                Line::from(vec![
                    Span::raw(self.cap_edit_buf.clone()),
                    Span::styled("▏", Style::default().fg(Color::Cyan)),
                ]),
            ];
            let editor = Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(focus_block("Edit goal", true));
            f.render_widget(editor, area);
            return;
        }

        let (title, lines) = match self.cap_detail.as_ref() {
            Some(detail) => (
                format!("Capsule — {}", detail.path),
                capsule_body_lines(detail),
            ),
            None => (
                "Capsule detail".to_string(),
                vec![Line::from(
                    "Move with ↑/↓; press Enter/→ on a capsule to open it here.",
                )],
            ),
        };
        let body = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.cap_scroll, 0))
            .block(focus_block(
                &title,
                focused && self.cap_focus == CapFocus::Detail,
            ));
        f.render_widget(body, area);
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
        let block = Block::default().borders(Borders::ALL).title("Token split");
        if total == 0 {
            f.render_widget(
                Paragraph::new("No usage logs found.").block(block),
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
        let lines = vec![
            Line::from(format!(
                "Total {}  ~${:.2}",
                human_tokens(total),
                self.usage.total.cost_usd
            )),
            Line::from(vec![
                Span::styled("● claude  ", Style::default().fg(CLAUDE_COLOR)),
                Span::raw(format!("{:>7}  {:>4.0}%", human_tokens(claude), pct(claude))),
            ]),
            Line::from(vec![
                Span::styled("● codex   ", Style::default().fg(CODEX_COLOR)),
                Span::raw(format!("{:>7}  {:>4.0}%", human_tokens(codex), pct(codex))),
            ]),
            Line::from("estimate — not an official bill").italic(),
        ];
        f.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
            area,
        );
    }

    fn draw_usage(&self, f: &mut Frame, area: Rect) {
        let dim = self.usage_dim;
        let groups = self.usage.breakdown(dim);
        let title = format!(
            "By {} — total {} tok ~${:.2} (g: change)",
            dim_name(dim),
            thousands(self.usage.total.tokens.total()),
            self.usage.total.cost_usd
        );
        if groups.is_empty() {
            let para = Paragraph::new("No usage logs found under ~/.claude/projects or ~/.codex.")
                .block(Block::default().borders(Borders::ALL).title(title));
            f.render_widget(para, area);
            return;
        }

        // Top: a token bar chart for the current dimension. Bottom: the table.
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(area);

        // Bar labels/values must outlive the BarChart, so own them here.
        let owned: Vec<(String, u64)> = chart_subset(dim, groups)
            .iter()
            .map(|g| (short_label(dim, &g.key), g.tokens.total()))
            .collect();
        let data: Vec<(&str, u64)> = owned.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let bar_width = bar_width_for(rows[0].width, data.len());
        let chart = BarChart::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("{title}  [tokens]")),
            )
            .data(data.as_slice())
            .bar_width(bar_width)
            .bar_gap(1)
            .bar_style(Style::default().fg(Color::Cyan))
            .value_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .label_style(Style::default().fg(Color::Gray));
        f.render_widget(chart, rows[0]);

        let table_rows = groups.iter().map(group_table_row);
        let table = Table::new(
            table_rows,
            [Constraint::Min(16), Constraint::Length(16), Constraint::Length(12), Constraint::Length(14)],
        )
        .header(Row::new(["Key", "Tokens", "Est $", "Unpriced"]).style(Style::default().add_modifier(Modifier::BOLD)))
        .block(Block::default().borders(Borders::ALL));
        f.render_widget(table, rows[1]);
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
            .header(Row::new(["Setting", "Value"]).style(Style::default().add_modifier(Modifier::BOLD)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Settings (↑/↓ select · space toggle · ←/→ change · applies to both agents)"),
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

fn group_table_row(g: &Group) -> Row<'static> {
    let unpriced = if g.unpriced_tokens > 0 {
        thousands(g.unpriced_tokens)
    } else {
        "-".to_string()
    };
    let key = if g.key.is_empty() { "(unknown)".to_string() } else { g.key.clone() };
    Row::new([
        Cell::from(key),
        Cell::from(thousands(g.tokens.total())),
        Cell::from(format!("{:.2}", g.cost_usd)),
        Cell::from(unpriced),
    ])
}

/// Pick which groups to chart: the most recent ~14 days for the Day dimension
/// (chronological), otherwise the top ~12 buckets (already tokens-desc).
fn chart_subset(dim: Dimension, groups: &[Group]) -> Vec<&Group> {
    match dim {
        // by_day is ascending; keep the last 14 in chronological order.
        Dimension::Day => {
            let start = groups.len().saturating_sub(14);
            groups[start..].iter().collect()
        }
        // already sorted tokens-desc; take the biggest 12.
        _ => groups.iter().take(12).collect(),
    }
}

/// Shorten a group key to fit under a bar.
fn short_label(dim: Dimension, key: &str) -> String {
    if key.is_empty() {
        return "?".to_string();
    }
    match dim {
        // YYYY-MM-DD -> MM-DD
        Dimension::Day => key.get(5..).unwrap_or(key).to_string(),
        // basename of a path
        Dimension::Project => key
            .rsplit(['/', '\\'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(key)
            .chars()
            .take(10)
            .collect(),
        // strip a vendor prefix, cap length
        Dimension::Model => key
            .rsplit_once('-')
            .map(|(_, tail)| tail)
            .filter(|t| t.len() >= 2)
            .unwrap_or(key)
            .chars()
            .take(10)
            .collect(),
        Dimension::Source => key.to_string(),
    }
}

/// Bar width that fits `n` bars (with 1-cell gaps) into `width`, min 3.
fn bar_width_for(width: u16, n: usize) -> u16 {
    if n == 0 {
        return 3;
    }
    let inner = width.saturating_sub(2); // borders
    let per = inner / n as u16;
    per.saturating_sub(1).clamp(3, 12)
}

/// Flip membership of `key` in `set` (insert if absent, remove if present).
fn toggle<T: Eq + std::hash::Hash>(set: &mut HashSet<T>, key: T) {
    if !set.remove(&key) {
        set.insert(key);
    }
}

/// A bordered block whose outline is highlighted (thick + yellow) when focused —
/// this is the "외곽선" that shows which pane the user is in.
fn focus_block(title: &str, focused: bool) -> Block<'static> {
    let (border_type, style) = if focused {
        (BorderType::Thick, Style::default().fg(Color::Yellow))
    } else {
        (BorderType::Plain, Style::default().fg(Color::DarkGray))
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(style)
        .title(title.to_string())
}

/// Style for an action-bar button: emphasised when its pane has focus.
fn action_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    }
}

/// Render the selected capsule as readable lines (or its raw text when it does
/// not parse as a v2 capsule).
fn capsule_body_lines(detail: &CapDetail) -> Vec<Line<'static>> {
    let Some(c) = detail.parsed.as_ref() else {
        return detail.raw.lines().map(|l| Line::from(l.to_string())).collect();
    };
    let head = |k: &str, v: String| {
        Line::from(vec![
            Span::styled(
                format!("{k}: "),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(v),
        ])
    };
    let state = match c.consumption.state {
        ai_handoff_core::capsule::ConsumptionState::Pending => "pending",
        ai_handoff_core::capsule::ConsumptionState::Consumed => "consumed",
    };
    let mut lines = vec![
        head("Goal", c.summary.goal.clone()),
        head("Flow", format!("{:?} → {:?}", c.source_agent, c.target_agent)),
        head("State", state.to_string()),
        head("Created", c.created_at.clone()),
        head("Capsule", c.capsule_id.clone()),
        Line::from(""),
    ];
    push_list(&mut lines, "Done", &c.summary.done);
    push_list(&mut lines, "Remaining", &c.summary.remaining);
    push_list(&mut lines, "Risks", &c.summary.risks);
    if let Some(np) = &c.next_prompt {
        lines.push(Line::from(""));
        lines.push(Line::from(
            Span::styled("Next prompt:", Style::default().add_modifier(Modifier::BOLD)),
        ));
        lines.extend(np.lines().map(|l| Line::from(l.to_string())));
    }
    if !c.files.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(
            Span::styled("Files:", Style::default().add_modifier(Modifier::BOLD)),
        ));
        for fch in &c.files {
            let status = fch.status.clone().unwrap_or_default();
            lines.push(Line::from(format!("  {status} {}", fch.path)));
        }
    }
    lines
}

/// Append a bold-labelled bullet list (skipped when empty).
fn push_list(lines: &mut Vec<Line<'static>>, label: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    lines.push(Line::from(Span::styled(
        format!("{label}:"),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.extend(items.iter().map(|it| Line::from(format!("  - {it}"))));
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

/// Group digits in threes with commas.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
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
    fn q_steps_back_through_visited_tabs_not_straight_to_overview() {
        let mut app = test_app();
        app.on_key(key(KeyCode::Char('2'))); // Overview -> Capsule
        app.on_key(key(KeyCode::Char('4'))); // Capsule -> Settings
        assert_eq!(app.tab, Tab::Settings);

        app.on_key(key(KeyCode::Char('q'))); // back to where we were: Capsule
        assert_eq!(app.tab, Tab::Capsule);
        assert!(!app.should_quit());

        app.on_key(key(KeyCode::Esc)); // back again: Overview
        assert_eq!(app.tab, Tab::Overview);
        assert!(!app.should_quit());
    }

    #[test]
    fn q_on_overview_arms_then_quits() {
        let mut app = test_app();
        assert_eq!(app.tab, Tab::Overview);
        app.on_key(key(KeyCode::Char('q')));
        assert!(!app.should_quit(), "first q must only arm");
        assert!(app.confirm_quit);
        assert!(app.status.contains("종료"));
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
        app.on_key(key(KeyCode::Char('q'))); // arm on Overview (empty history)
        assert!(app.confirm_quit);
        app.on_key(key(KeyCode::Tab)); // any other key disarms + navigates
        assert!(!app.confirm_quit);
        assert_eq!(app.tab, Tab::Capsule);
        // q steps back to Overview (history pop), not a quit
        app.on_key(key(KeyCode::Char('q')));
        assert_eq!(app.tab, Tab::Overview);
        assert!(!app.should_quit());
        // now at root: q arms again
        app.on_key(key(KeyCode::Char('q')));
        assert!(!app.should_quit());
        assert!(app.confirm_quit);
    }

    #[test]
    fn chart_subset_keeps_recent_days_and_top_others() {
        use ai_handoff_usage::aggregate::Group;
        let days: Vec<Group> = (1..=20)
            .map(|d| Group {
                key: format!("2026-06-{d:02}"),
                tokens: Default::default(),
                cost_usd: 0.0,
                unpriced_tokens: 0,
                events: 1,
            })
            .collect();
        let subset = chart_subset(Dimension::Day, &days);
        assert_eq!(subset.len(), 14);
        assert_eq!(subset.last().unwrap().key, "2026-06-20"); // newest kept, chronological
    }

    #[test]
    fn short_label_shortens_per_dimension() {
        assert_eq!(short_label(Dimension::Day, "2026-06-17"), "06-17");
        assert_eq!(short_label(Dimension::Project, "C:/code/my-proj"), "my-proj");
        assert_eq!(short_label(Dimension::Source, "codex"), "codex");
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
    fn g_cycles_usage_dimension() {
        let mut app = test_app();
        app.tab = Tab::Usage;
        app.focus_content = true; // 'g' only acts once inside the tab content
        assert_eq!(app.usage_dim, Dimension::Day);
        app.on_key(key(KeyCode::Char('g')));
        assert_eq!(app.usage_dim, Dimension::Model);
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
        assert!(app.status.contains("saved"));
        // the write actually landed
        let cfg = ai_handoff_core::config::load_from(&app.config_path);
        let val = ai_handoff_core::config::get_value(&cfg, first_key).unwrap();
        assert_eq!(val, "false");
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
