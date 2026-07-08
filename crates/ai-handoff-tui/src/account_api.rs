//! Per-account provider usage. The implementation now lives in
//! `ai_handoff_core::account_api` (so the daemon can reach it too); this
//! re-export keeps `ai_handoff_tui::account_api::…` paths working for the TUI,
//! CLI, and desktop callers.
pub use ai_handoff_core::account_api::*;
