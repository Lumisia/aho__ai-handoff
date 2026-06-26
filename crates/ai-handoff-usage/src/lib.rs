//! ai-handoff-usage — local token-accounting engine.
//!
//! Pure, in-process library: scan Claude and Codex JSONL logs, normalize each
//! turn into a [`model::UsageEvent`], and aggregate by day / model / project /
//! source with an estimated (never official) USD cost. No daemon, no network,
//! read-only.

pub mod model;
pub mod pricing;
