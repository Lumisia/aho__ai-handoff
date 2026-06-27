//! ai-handoff-tui — the ratatui terminal dashboard.
//!
//! Overview (health + token summary), Capsule (handoff capsules), Account
//! (Codex/Claude plan + 5h/weekly limits + reset credits, with a saved-account
//! switch pool), and Settings (edit the shared config, applied to both agents).
//! Reads the usage engine and config in-process at launch. The bare
//! `ai-handoff` / `ai-handoff tui` invocation opens this; hook commands never do.

pub mod account_api;
pub mod app;
pub mod capsule_ops;
pub mod edit;
pub mod viewmodel;

pub use app::App;

// Load the shared workspace translations (en/ko/ja/zh) at compile time. The
// active locale is global (rust_i18n::set_locale) and shared across crates.
rust_i18n::i18n!("../../locales", fallback = "en");

/// Point rust-i18n at the configured UI language (call before rendering).
pub fn apply_language(cfg: &ai_handoff_core::config::Config) {
    rust_i18n::set_locale(ai_handoff_core::config::lang_str(cfg.language));
}

/// Launch the interactive TUI: set up the terminal, run the event loop, and
/// restore the terminal on exit (even on error).
pub fn run() -> std::io::Result<()> {
    apply_language(&ai_handoff_core::config::load());
    let mut terminal = ratatui::init();
    let mut app = App::load();
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}
