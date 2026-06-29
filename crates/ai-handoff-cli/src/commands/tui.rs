use std::io::{IsTerminal, Write};

use ai_handoff_core::dashboard::{CheckStatus, DashboardSnapshot};

pub fn run() -> anyhow::Result<i32> {
    // Interactive terminal: launch the ratatui dashboard. Otherwise (piped or
    // redirected output) fall back to the static text snapshot below — this
    // also keeps `ai-handoff | cat` and tests from trying to open a TUI.
    if std::io::stdout().is_terminal() {
        ai_handoff_tui::run()?;
        return Ok(0);
    }
    let snapshot = ai_handoff_core::dashboard::dashboard_snapshot();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    run_io(&snapshot, &mut out)
}

pub fn run_io(snapshot: &DashboardSnapshot, out: &mut dyn Write) -> anyhow::Result<i32> {
    writeln!(out, "AI Handoff")?;
    writeln!(out, "Terminal dashboard")?;
    writeln!(out)?;
    writeln!(out, "Capsules")?;
    writeln!(out, "  pending: {}", snapshot.capsules.pending_count)?;
    writeln!(out, "  total: {}", snapshot.capsules.items.len())?;
    writeln!(out)?;
    writeln!(out, "Health")?;
    for check in &snapshot.checks {
        writeln!(
            out,
            "  {:<16} {:<8} {}",
            check.label,
            status_text(&check.status),
            check.message
        )?;
    }
    writeln!(out)?;
    writeln!(out, "Usage")?;
    writeln!(
        out,
        "  ai-handoff usage shows estimated token usage (day/model/project)"
    )?;
    writeln!(out)?;
    writeln!(out, "Next")?;
    writeln!(out, "  ai-handoff --help shows subcommands")?;
    writeln!(
        out,
        "  ai-handoff usage --group-by model breaks down tokens"
    )?;
    writeln!(out, "  ai-handoff dashboard opens the optional GUI")?;
    Ok(0)
}

fn status_text(status: &CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "ok",
        CheckStatus::Warning => "warning",
        CheckStatus::Error => "error",
        CheckStatus::Missing => "missing",
        CheckStatus::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_core::dashboard::dashboard_snapshot_for;

    #[test]
    fn terminal_dashboard_renders_current_snapshot_and_next_steps() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = dashboard_snapshot_for(dir.path(), dir.path());
        let mut out = Vec::new();

        let code = run_io(&snapshot, &mut out).unwrap();

        assert_eq!(code, 0);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("AI Handoff"));
        assert!(text.contains("Terminal dashboard"));
        assert!(text.contains("Capsules"));
        assert!(text.contains("Health"));
        assert!(text.contains("ai-handoff usage shows estimated token usage"));
        assert!(text.contains("ai-handoff --help"));
    }
}
