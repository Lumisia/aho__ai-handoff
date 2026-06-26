//! ai-handoff-tui — the ratatui terminal dashboard.
//!
//! Overview (health + token summary), Usage (day/model/project/source
//! breakdown), and Settings (edit the shared config, applied to both agents).
//! Reads the usage engine and config in-process at launch. The bare
//! `ai-handoff` / `ai-handoff tui` invocation opens this; hook commands never do.

pub mod app;
pub mod capsule_ops;
pub mod edit;
pub mod viewmodel;

pub use app::App;

/// Launch the interactive TUI: set up the terminal, run the event loop, and
/// restore the terminal on exit (even on error).
pub fn run() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::load();
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}
