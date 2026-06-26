//! Group [`UsageEvent`]s by a dimension and attach an estimated cost.
//!
//! Cost is computed per event from its own model (so a mixed group is priced
//! correctly), then summed. Tokens from models with no price entry are counted
//! into `unpriced_tokens` so the UI can show "+N tokens unpriced" rather than
//! silently understating cost.

use std::collections::HashMap;

use serde::Serialize;

use crate::model::{Source, Tokens, UsageEvent};
use crate::pricing;

/// A breakdown axis for [`group_by`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Day,
    Model,
    Project,
    Source,
}

impl Dimension {
    fn key_of(self, e: &UsageEvent) -> String {
        match self {
            Dimension::Day => e.day.clone(),
            Dimension::Model => e.model.clone(),
            Dimension::Project => e.project.clone(),
            Dimension::Source => e.source.as_str().to_string(),
        }
    }
}

/// One aggregated bucket.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Group {
    pub key: String,
    pub tokens: Tokens,
    /// Estimated USD across priced events in this group.
    pub cost_usd: f64,
    /// Total token volume of events whose model had no known price.
    pub unpriced_tokens: u64,
    /// Number of usage events in this group.
    pub events: u64,
}

impl Group {
    fn new(key: String) -> Self {
        Group {
            key,
            tokens: Tokens::default(),
            cost_usd: 0.0,
            unpriced_tokens: 0,
            events: 0,
        }
    }

    fn add(&mut self, e: &UsageEvent) {
        self.tokens.add(&e.tokens);
        self.events += 1;
        match event_cost(e) {
            Some(c) => self.cost_usd += c,
            None => self.unpriced_tokens += e.tokens.total(),
        }
    }
}

/// Estimated USD cost of one event, or `None` if its model is unpriced.
pub fn event_cost(e: &UsageEvent) -> Option<f64> {
    pricing::estimate_cost(&e.model, &e.tokens)
}

/// Group events by `dim`, sorted by total tokens descending (ties by key).
pub fn group_by(events: &[UsageEvent], dim: Dimension) -> Vec<Group> {
    let mut map: HashMap<String, Group> = HashMap::new();
    for e in events {
        let key = dim.key_of(e);
        map.entry(key.clone())
            .or_insert_with(|| Group::new(key))
            .add(e);
    }
    let mut groups: Vec<Group> = map.into_values().collect();
    groups.sort_by(|a, b| {
        b.tokens
            .total()
            .cmp(&a.tokens.total())
            .then_with(|| a.key.cmp(&b.key))
    });
    groups
}

/// A single grand-total bucket over all events (`key = "total"`).
pub fn totals(events: &[UsageEvent]) -> Group {
    let mut g = Group::new("total".to_string());
    for e in events {
        g.add(e);
    }
    g
}

/// Keep only events on or after `since_day` (`YYYY-MM-DD`, lexicographic).
pub fn filter_since(events: Vec<UsageEvent>, since_day: &str) -> Vec<UsageEvent> {
    events
        .into_iter()
        .filter(|e| e.day.as_str() >= since_day)
        .collect()
}

/// Keep only events from `source`.
pub fn filter_source(events: Vec<UsageEvent>, source: Source) -> Vec<UsageEvent> {
    events.into_iter().filter(|e| e.source == source).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(source: Source, project: &str, model: &str, day: &str, t: Tokens) -> UsageEvent {
        UsageEvent {
            source,
            project: project.into(),
            session: "s".into(),
            model: model.into(),
            day: day.into(),
            tokens: t,
        }
    }

    fn sample() -> Vec<UsageEvent> {
        vec![
            ev(Source::Claude, "C:/a", "claude-opus-4-8", "2026-06-17", Tokens { input: 100, cache_read: 0, cache_write: 0, output: 10 }),
            ev(Source::Claude, "C:/a", "claude-opus-4-8", "2026-06-18", Tokens { input: 200, cache_read: 0, cache_write: 0, output: 20 }),
            ev(Source::Codex, "C:/b", "gpt-5.5", "2026-06-18", Tokens { input: 50, cache_read: 5, cache_write: 0, output: 5 }),
            ev(Source::Codex, "C:/b", "mystery-model", "2026-06-18", Tokens { input: 1000, cache_read: 0, cache_write: 0, output: 0 }),
        ]
    }

    #[test]
    fn group_by_source_splits_and_sorts_by_tokens() {
        let g = group_by(&sample(), Dimension::Source);
        assert_eq!(g.len(), 2);
        // codex has 50+5+5 + 1000 = 1060 tokens vs claude 330 -> codex first.
        assert_eq!(g[0].key, "codex");
        assert_eq!(g[0].tokens.total(), 1060);
        assert_eq!(g[1].key, "claude");
        assert_eq!(g[1].tokens.total(), 330);
    }

    #[test]
    fn group_by_day_buckets_correctly() {
        let g = group_by(&sample(), Dimension::Day);
        let day18 = g.iter().find(|x| x.key == "2026-06-18").unwrap();
        assert_eq!(day18.events, 3);
    }

    #[test]
    fn group_by_project_and_model() {
        assert_eq!(group_by(&sample(), Dimension::Project).len(), 2);
        assert_eq!(group_by(&sample(), Dimension::Model).len(), 3);
    }

    #[test]
    fn unpriced_model_counts_tokens_but_not_cost() {
        let g = group_by(&sample(), Dimension::Model);
        let mystery = g.iter().find(|x| x.key == "mystery-model").unwrap();
        assert_eq!(mystery.cost_usd, 0.0);
        assert_eq!(mystery.unpriced_tokens, 1000);
    }

    #[test]
    fn totals_sum_all_and_track_unpriced() {
        let t = totals(&sample());
        assert_eq!(t.tokens.total(), 330 + 1060);
        assert_eq!(t.unpriced_tokens, 1000);
        assert!(t.cost_usd > 0.0); // opus + gpt priced
    }

    #[test]
    fn filter_since_is_inclusive_lexicographic() {
        let kept = filter_since(sample(), "2026-06-18");
        assert_eq!(kept.len(), 3);
        assert!(kept.iter().all(|e| e.day.as_str() >= "2026-06-18"));
    }

    #[test]
    fn filter_source_selects_one_agent() {
        assert_eq!(filter_source(sample(), Source::Claude).len(), 2);
        assert_eq!(filter_source(sample(), Source::Codex).len(), 2);
    }
}
