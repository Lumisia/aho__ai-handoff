//! `ai-handoff usage` — estimated token usage from local Claude + Codex logs.
//!
//! Read-only, in-process: scan the log roots, optionally filter by source/since,
//! then either print a summary (default) or a breakdown by day/model/project/
//! source. All costs are local estimates, never an official bill.

use std::io::Write;

use ai_handoff_usage::{
    aggregate::{self, Group},
    engine,
    model::{Source, UsageEvent},
    Dimension,
};
use serde_json::json;

use crate::{GroupByArg, SourceArg};

const ESTIMATE_NOTE: &str =
    "Estimated from local logs — not an official bill or quota. Costs are approximate.";

pub fn run(
    group_by: Option<GroupByArg>,
    source: Option<SourceArg>,
    since: Option<String>,
    json_output: bool,
) -> anyhow::Result<i32> {
    let mut events = engine::scan_default();
    if let Some(s) = source {
        events = aggregate::filter_source(events, map_source(s));
    }
    if let Some(day) = &since {
        events = aggregate::filter_since(events, day);
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    render(&events, group_by.map(map_dim), json_output, &mut out)?;
    Ok(0)
}

fn map_dim(g: GroupByArg) -> Dimension {
    match g {
        GroupByArg::Day => Dimension::Day,
        GroupByArg::Model => Dimension::Model,
        GroupByArg::Project => Dimension::Project,
        GroupByArg::Source => Dimension::Source,
    }
}

fn map_source(s: SourceArg) -> Source {
    match s {
        SourceArg::Claude => Source::Claude,
        SourceArg::Codex => Source::Codex,
    }
}

/// Render the report. Pure over `events` so it is unit-testable without IO.
pub fn render(
    events: &[UsageEvent],
    group_by: Option<Dimension>,
    json_output: bool,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let total = aggregate::totals(events);

    if json_output {
        let mut value = json!({
            "estimate_note": ESTIMATE_NOTE,
            "total": &total,
        });
        match group_by {
            Some(dim) => {
                value["group_by"] = json!(dim_name(dim));
                value["groups"] = json!(aggregate::group_by(events, dim));
            }
            None => {
                value["by_source"] = json!(aggregate::group_by(events, Dimension::Source));
            }
        }
        writeln!(out, "{}", serde_json::to_string_pretty(&value)?)?;
        return Ok(());
    }

    writeln!(out, "AI Handoff — token usage")?;
    writeln!(out, "{ESTIMATE_NOTE}")?;
    writeln!(out)?;

    if events.is_empty() {
        writeln!(out, "No usage logs found.")?;
        writeln!(
            out,
            "Looked under ~/.claude/projects and ~/.codex/sessions."
        )?;
        return Ok(());
    }

    writeln!(out, "Total: {}", summarize(&total))?;
    writeln!(out)?;

    match group_by {
        Some(dim) => {
            writeln!(out, "By {}:", dim_name(dim))?;
            for g in aggregate::group_by(events, dim) {
                let label = if g.key.is_empty() {
                    "(unknown)"
                } else {
                    &g.key
                };
                writeln!(out, "  {:<28} {}", label, summarize(&g))?;
            }
        }
        None => {
            for g in aggregate::group_by(events, Dimension::Source) {
                writeln!(out, "  {:<10} {}", g.key, summarize(&g))?;
            }
        }
    }
    Ok(())
}

fn dim_name(dim: Dimension) -> &'static str {
    match dim {
        Dimension::Day => "day",
        Dimension::Model => "model",
        Dimension::Project => "project",
        Dimension::Source => "source",
    }
}

/// `"1,234,567 tokens   ~$12.34 (est)[ +N unpriced]"`.
fn summarize(g: &Group) -> String {
    let mut s = format!(
        "{:>14} tokens   ~${:.2} (est)",
        thousands(g.tokens.total()),
        g.cost_usd
    );
    if g.unpriced_tokens > 0 {
        s.push_str(&format!(" (+{} unpriced)", thousands(g.unpriced_tokens)));
    }
    s
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
    use ai_handoff_usage::model::{Source, Tokens};

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

    fn sample() -> Vec<UsageEvent> {
        vec![
            ev(
                Source::Claude,
                "claude-opus-4-8",
                "2026-06-17",
                Tokens {
                    input: 1_000_000,
                    cache_read: 0,
                    cache_write: 0,
                    output: 1_000_000,
                },
            ),
            ev(
                Source::Codex,
                "mystery",
                "2026-06-18",
                Tokens {
                    input: 500,
                    cache_read: 0,
                    cache_write: 0,
                    output: 0,
                },
            ),
        ]
    }

    fn render_text(events: &[UsageEvent], group_by: Option<Dimension>) -> String {
        let mut out = Vec::new();
        render(events, group_by, false, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn default_summary_shows_total_and_per_source() {
        let text = render_text(&sample(), None);
        assert!(text.contains("token usage"));
        assert!(text.contains("not an official bill"));
        assert!(text.contains("Total:"));
        assert!(text.contains("claude"));
        assert!(text.contains("codex"));
        // opus 1M in + 1M out = $15 + $75 = $90.00
        assert!(text.contains("$90.00"), "got: {text}");
        // mystery model tokens are unpriced
        assert!(text.contains("unpriced"));
    }

    #[test]
    fn group_by_model_lists_models() {
        let text = render_text(&sample(), Some(Dimension::Model));
        assert!(text.contains("By model:"));
        assert!(text.contains("claude-opus-4-8"));
        assert!(text.contains("mystery"));
    }

    #[test]
    fn empty_events_report_no_logs() {
        let text = render_text(&[], None);
        assert!(text.contains("No usage logs found"));
    }

    #[test]
    fn json_output_is_structured_and_parseable() {
        let mut out = Vec::new();
        render(&sample(), Some(Dimension::Source), true, &mut out).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["group_by"], "source");
        assert!(v["estimate_note"]
            .as_str()
            .unwrap()
            .contains("not an official"));
        assert!(v["total"]["tokens"]["input"].as_u64().unwrap() >= 1_000_000);
        assert!(v["groups"].as_array().unwrap().len() == 2);
    }

    #[test]
    fn empty_project_key_renders_as_unknown() {
        let mut e = sample();
        e[0].project = String::new();
        let text = render_text(&e, Some(Dimension::Project));
        assert!(text.contains("(unknown)"));
    }
}
