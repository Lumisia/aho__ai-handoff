//! Pure view-models: shape engine/core data into rows the render layer draws.
//! Kept free of ratatui/crossterm so it is unit-testable without a terminal.

use ai_handoff_core::config::{self, Config, KeyKind};
use ai_handoff_core::dashboard::{CapsuleList, CapsuleSummary, CheckStatus, DashboardSnapshot};
use ai_handoff_usage::{
    aggregate::{self, Group},
    model::UsageEvent,
    Dimension,
};

/// Aggregated usage for the Usage tab.
#[derive(Debug, Clone)]
pub struct UsageView {
    pub total: Group,
    pub by_source: Vec<Group>,
    /// Chronological (ascending day) for the per-day bars.
    pub by_day: Vec<Group>,
    pub by_model: Vec<Group>,
    pub by_project: Vec<Group>,
}

impl UsageView {
    pub fn from_events(events: &[UsageEvent]) -> Self {
        let mut by_day = aggregate::group_by(events, Dimension::Day);
        by_day.sort_by(|a, b| a.key.cmp(&b.key));
        UsageView {
            total: aggregate::totals(events),
            by_source: aggregate::group_by(events, Dimension::Source),
            by_day,
            by_model: aggregate::group_by(events, Dimension::Model),
            by_project: aggregate::group_by(events, Dimension::Project),
        }
    }

    /// The breakdown for a given dimension (used by the toggle in the Usage tab).
    pub fn breakdown(&self, dim: Dimension) -> &[Group] {
        match dim {
            Dimension::Day => &self.by_day,
            Dimension::Model => &self.by_model,
            Dimension::Project => &self.by_project,
            Dimension::Source => &self.by_source,
        }
    }
}

/// One health row for the Overview tab.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthRow {
    pub label: String,
    pub status: CheckStatus,
    pub detail: String,
}

/// Flatten a dashboard snapshot into compact health rows, translating the
/// well-known check labels (by id) and fixed messages (by text). Dynamic
/// messages (paths, parse errors, value dumps) pass through untranslated.
pub fn health_rows(snapshot: &DashboardSnapshot) -> Vec<HealthRow> {
    snapshot
        .checks
        .iter()
        .map(|c| HealthRow {
            label: health_label(&c.id, &c.label),
            status: c.status.clone(),
            detail: health_detail(&c.message),
        })
        .collect()
}

fn health_label(id: &str, fallback: &str) -> String {
    let key = match id {
        "daemon" => "health.label.daemon",
        "autostart" => "health.label.autostart",
        "codex-hooks" => "health.label.codex-hooks",
        "codex-config" => "health.label.codex-config",
        "claude-settings" => "health.label.claude-settings",
        "ipc" => "health.label.ipc",
        "store" => "health.label.store",
        _ => return fallback.to_string(),
    };
    rust_i18n::t!(key).into_owned()
}

fn health_detail(message: &str) -> String {
    let key = match message {
        "present" => "health.msg.present",
        "missing" => "health.msg.missing",
        "path exists but is not a directory" => "health.msg.not_a_dir",
        "v2 hooks installed" => "health.msg.hooks_installed",
        "v2 hooks missing or incomplete" => "health.msg.hooks_incomplete",
        "writable_roots and AI_HANDOFF_HOME present" => "health.msg.config_ok",
        "Runtime status API not implemented in this MVP" => "health.msg.daemon_unknown",
        _ => return message.to_string(),
    };
    rust_i18n::t!(key).into_owned()
}

/// Capsule tab tree: one connected agent (Codex / Claude) and its projects.
#[derive(Debug, Clone, PartialEq)]
pub struct CapsuleAgent {
    pub agent: String,
    pub projects: Vec<CapsuleProject>,
    pub count: usize,
}

/// One project under an agent, with the capsules captured for it.
#[derive(Debug, Clone, PartialEq)]
pub struct CapsuleProject {
    pub project_id: String,
    pub capsules: Vec<CapsuleSummary>,
}

/// Group a flat capsule list into agent → project → capsule, preserving the
/// first-seen order of agents and projects (the list itself is created-desc).
pub fn capsule_tree(list: &CapsuleList) -> Vec<CapsuleAgent> {
    let mut agents: Vec<CapsuleAgent> = Vec::new();
    for item in &list.items {
        let ai = match agents.iter().position(|a| a.agent == item.source_agent) {
            Some(i) => i,
            None => {
                agents.push(CapsuleAgent {
                    agent: item.source_agent.clone(),
                    projects: Vec::new(),
                    count: 0,
                });
                agents.len() - 1
            }
        };
        let agent = &mut agents[ai];
        agent.count += 1;
        match agent.projects.iter().position(|p| p.project_id == item.project_id) {
            Some(pi) => agent.projects[pi].capsules.push(item.clone()),
            None => agent.projects.push(CapsuleProject {
                project_id: item.project_id.clone(),
                capsules: vec![item.clone()],
            }),
        }
    }
    agents
}

/// One editable setting row for the Settings tab.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingRow {
    pub key: &'static str,
    pub value: String,
    pub kind: KeyKind,
}

/// Build the Settings rows from a resolved config (effective values).
pub fn settings_rows(cfg: &Config) -> Vec<SettingRow> {
    config::settable_keys()
        .filter_map(|key| {
            let kind = config::key_kind(key)?;
            let value = config::get_value(cfg, key).ok()?;
            Some(SettingRow { key, value, kind })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_usage::model::{Source, Tokens, UsageEvent};

    fn ev(source: Source, model: &str, day: &str, t: Tokens) -> UsageEvent {
        UsageEvent {
            source,
            project: "C:/p".into(),
            session: "s".into(),
            model: model.into(),
            day: day.into(),
            tokens: t,
        }
    }

    #[test]
    fn usage_view_orders_days_ascending() {
        let events = vec![
            ev(Source::Codex, "gpt-5.5", "2026-06-18", Tokens { input: 5, ..Default::default() }),
            ev(Source::Codex, "gpt-5.5", "2026-06-16", Tokens { input: 5, ..Default::default() }),
            ev(Source::Codex, "gpt-5.5", "2026-06-17", Tokens { input: 5, ..Default::default() }),
        ];
        let v = UsageView::from_events(&events);
        let days: Vec<&str> = v.by_day.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(days, ["2026-06-16", "2026-06-17", "2026-06-18"]);
    }

    #[test]
    fn breakdown_selects_the_right_dimension() {
        let events = vec![ev(Source::Claude, "claude-opus-4-8", "2026-06-17", Tokens { input: 1, ..Default::default() })];
        let v = UsageView::from_events(&events);
        assert_eq!(v.breakdown(Dimension::Model).len(), 1);
        assert_eq!(v.breakdown(Dimension::Source)[0].key, "claude");
    }

    #[test]
    fn capsule_tree_groups_by_agent_then_project() {
        let cap = |agent: &str, project: &str, id: &str| CapsuleSummary {
            capsule_id: id.into(),
            project_id: project.into(),
            created_at: "2026-06-25T01:01:01Z".into(),
            source_agent: agent.into(),
            target_agent: "Codex".into(),
            state: "pending".into(),
            summary_preview: "goal".into(),
            path: format!("/store/{project}/{id}.json"),
        };
        let list = CapsuleList {
            items: vec![
                cap("Codex", "proj-a", "c1"),
                cap("Codex", "proj-a", "c2"),
                cap("Codex", "proj-b", "c3"),
                cap("ClaudeCode", "proj-a", "c4"),
            ],
            pending_count: 4,
            skipped: 0,
        };
        let tree = capsule_tree(&list);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].agent, "Codex");
        assert_eq!(tree[0].count, 3);
        assert_eq!(tree[0].projects.len(), 2);
        assert_eq!(tree[0].projects[0].project_id, "proj-a");
        assert_eq!(tree[0].projects[0].capsules.len(), 2);
        assert_eq!(tree[1].agent, "ClaudeCode");
        assert_eq!(tree[1].projects.len(), 1);
    }

    #[test]
    fn settings_rows_cover_all_keys_with_kinds() {
        let rows = settings_rows(&Config::default());
        assert_eq!(rows.len(), 8);
        let threshold = rows
            .iter()
            .find(|r| r.key == "triggers.five_hour.threshold_percent")
            .unwrap();
        assert_eq!(threshold.value, "80");
        assert_eq!(threshold.kind, KeyKind::Percent);
        let mode = rows.iter().find(|r| r.key == "triggers.five_hour.mode").unwrap();
        assert_eq!(mode.value, "ask");
        assert_eq!(mode.kind, KeyKind::Mode);
    }
}
