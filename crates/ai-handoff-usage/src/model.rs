//! Normalized usage model shared by the Claude and Codex parsers.
//!
//! The two agents log token counts in different shapes. We normalize both into
//! [`UsageEvent`] with four **disjoint** token buckets so a single pricing
//! formula applies to both and `input + cache_read + cache_write + output` is
//! always the event's total token volume:
//!
//! - Claude: `usage.input_tokens` is already cache-exclusive, with separate
//!   `cache_read_input_tokens` and `cache_creation_input_tokens`.
//! - Codex: `last_token_usage.input_tokens` **includes** `cached_input_tokens`,
//!   so fresh input = `input_tokens - cached_input_tokens`; Codex has no
//!   cache-creation bucket; `output_tokens` already includes reasoning tokens.

use serde::Serialize;

/// Which agent produced an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Claude,
    Codex,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Claude => "claude",
            Source::Codex => "codex",
        }
    }
}

/// Normalized, disjoint token counts for one usage event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Tokens {
    /// Fresh (non-cached) input tokens — billed at the model's input rate.
    pub input: u64,
    /// Cached input tokens — billed at the cache-read rate.
    pub cache_read: u64,
    /// Cache-creation (write) tokens — billed at the cache-write rate. Claude only.
    pub cache_write: u64,
    /// Output tokens — billed at the output rate (includes Codex reasoning tokens).
    pub output: u64,
}

impl Tokens {
    /// Total token volume across all four buckets.
    pub fn total(&self) -> u64 {
        self.input + self.cache_read + self.cache_write + self.output
    }

    /// Accumulate another event's tokens into this one.
    pub fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.output += other.output;
    }
}

/// One normalized usage event: a single assistant turn (Claude) or one
/// `token_count` delta (Codex).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageEvent {
    pub source: Source,
    /// The project directory (`cwd`) the turn ran in.
    pub project: String,
    /// Session id (Codex) or file stem / sessionId (Claude).
    pub session: String,
    /// The model slug, e.g. `claude-opus-4-8`, `gpt-5.5`.
    pub model: String,
    /// Local calendar day, `YYYY-MM-DD`.
    pub day: String,
    pub tokens: Tokens,
}

/// Convert an RFC3339 timestamp to a local `YYYY-MM-DD` day string.
/// Returns `None` for an unparseable timestamp.
pub fn local_day(timestamp: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_total_sums_all_four_buckets() {
        let t = Tokens {
            input: 100,
            cache_read: 50,
            cache_write: 10,
            output: 40,
        };
        assert_eq!(t.total(), 200);
    }

    #[test]
    fn tokens_add_accumulates() {
        let mut a = Tokens {
            input: 1,
            cache_read: 2,
            cache_write: 3,
            output: 4,
        };
        a.add(&Tokens {
            input: 10,
            cache_read: 20,
            cache_write: 30,
            output: 40,
        });
        assert_eq!(a, Tokens { input: 11, cache_read: 22, cache_write: 33, output: 44 });
    }

    #[test]
    fn source_as_str() {
        assert_eq!(Source::Claude.as_str(), "claude");
        assert_eq!(Source::Codex.as_str(), "codex");
    }

    #[test]
    fn local_day_parses_rfc3339() {
        // A valid timestamp resolves to some YYYY-MM-DD; the exact day depends
        // on the machine timezone, so just assert the shape.
        let day = local_day("2026-06-17T14:12:08.827Z").unwrap();
        assert_eq!(day.len(), 10);
        assert_eq!(day.matches('-').count(), 2);
    }

    #[test]
    fn local_day_rejects_garbage() {
        assert!(local_day("not a timestamp").is_none());
        assert!(local_day("").is_none());
    }

    #[test]
    fn usage_event_serializes_with_source_lowercase() {
        let ev = UsageEvent {
            source: Source::Codex,
            project: "C:/p".into(),
            session: "s1".into(),
            model: "gpt-5.5".into(),
            day: "2026-06-17".into(),
            tokens: Tokens { input: 5, cache_read: 1, cache_write: 0, output: 2 },
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["source"], "codex");
        assert_eq!(json["tokens"]["input"], 5);
    }
}
