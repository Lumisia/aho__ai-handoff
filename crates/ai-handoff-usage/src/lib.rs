//! ai-handoff-usage — local token-accounting engine.
//!
//! Pure, in-process library: scan Claude and Codex JSONL logs, normalize each
//! turn into a [`model::UsageEvent`], and aggregate by day / model / project /
//! source with an estimated (never official) USD cost. No daemon, no network,
//! read-only.

pub mod aggregate;
pub mod claude;
pub mod codex;
pub mod engine;
pub mod model;
pub mod pricing;

pub use aggregate::{group_by, totals, Dimension, Group};
pub use engine::{default_roots, scan, scan_default, Roots};
pub use model::{Source, Tokens, UsageEvent};
