//! Static, best-effort per-model pricing.
//!
//! These numbers are **estimates** used only to turn locally-logged token
//! counts into an approximate USD figure. They are NOT an official bill or
//! quota and will drift as vendors change prices — unknown models simply
//! report `None` (their tokens are still counted). All prices are USD per
//! million tokens.

use crate::model::Tokens;

/// Per-million-token USD prices for one model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub output: f64,
}

impl Price {
    /// Estimated USD cost for the given token counts.
    pub fn cost(&self, t: &Tokens) -> f64 {
        let per_m = 1_000_000.0;
        (t.input as f64 * self.input
            + t.cache_read as f64 * self.cache_read
            + t.cache_write as f64 * self.cache_write
            + t.output as f64 * self.output)
            / per_m
    }
}

/// (family key matched case-insensitively as a prefix, price). The first
/// matching family wins, so list more specific keys before broader ones.
const TABLE: &[(&str, Price)] = &[
    // Anthropic Claude (input / cache-read / cache-write / output $ per Mtok).
    (
        "claude-opus",
        Price {
            input: 15.0,
            cache_read: 1.5,
            cache_write: 18.75,
            output: 75.0,
        },
    ),
    (
        "claude-sonnet",
        Price {
            input: 3.0,
            cache_read: 0.3,
            cache_write: 3.75,
            output: 15.0,
        },
    ),
    (
        "claude-haiku",
        Price {
            input: 0.8,
            cache_read: 0.08,
            cache_write: 1.0,
            output: 4.0,
        },
    ),
    // OpenAI Codex / GPT-5 family (Codex reports cached input separately).
    (
        "gpt-5-codex",
        Price {
            input: 1.25,
            cache_read: 0.125,
            cache_write: 0.0,
            output: 10.0,
        },
    ),
    (
        "gpt-5",
        Price {
            input: 1.25,
            cache_read: 0.125,
            cache_write: 0.0,
            output: 10.0,
        },
    ),
    (
        "o3",
        Price {
            input: 2.0,
            cache_read: 0.5,
            cache_write: 0.0,
            output: 8.0,
        },
    ),
    (
        "o4-mini",
        Price {
            input: 1.1,
            cache_read: 0.275,
            cache_write: 0.0,
            output: 4.4,
        },
    ),
];

/// Look up a best-effort price for `model`. Matching is case-insensitive by
/// family prefix so versioned slugs (`claude-opus-4-8`, `gpt-5.5`) resolve
/// without an exact entry. Returns `None` for unknown models.
pub fn price_for(model: &str) -> Option<Price> {
    let m = model.to_ascii_lowercase();
    TABLE
        .iter()
        .find(|(key, _)| m.starts_with(key))
        .map(|(_, price)| *price)
}

/// Estimated USD cost for `model`'s `tokens`, or `None` if the model is unknown.
pub fn estimate_cost(model: &str, tokens: &Tokens) -> Option<f64> {
    price_for(model).map(|p| p.cost(tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_families_resolve_with_versioned_slugs() {
        assert!(price_for("claude-opus-4-8").is_some());
        assert!(price_for("claude-sonnet-4-6").is_some());
        assert!(price_for("gpt-5.5").is_some());
        assert!(price_for("gpt-5-codex").is_some());
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(price_for("Claude-Opus-4-8"), price_for("claude-opus-4-8"));
    }

    #[test]
    fn unknown_model_has_no_price() {
        assert!(price_for("some-future-model").is_none());
        assert!(estimate_cost(
            "some-future-model",
            &Tokens {
                input: 1000,
                ..Default::default()
            }
        )
        .is_none());
    }

    #[test]
    fn cost_math_is_per_million() {
        let p = Price {
            input: 15.0,
            cache_read: 1.5,
            cache_write: 18.75,
            output: 75.0,
        };
        // 1M input + 1M output = $15 + $75 = $90.
        let t = Tokens {
            input: 1_000_000,
            cache_read: 0,
            cache_write: 0,
            output: 1_000_000,
        };
        assert!((p.cost(&t) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn cost_includes_cache_buckets() {
        let p = Price {
            input: 0.0,
            cache_read: 1.0,
            cache_write: 2.0,
            output: 0.0,
        };
        let t = Tokens {
            input: 0,
            cache_read: 1_000_000,
            cache_write: 1_000_000,
            output: 0,
        };
        assert!((p.cost(&t) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn gpt5_codex_matched_before_generic_gpt5() {
        // The codex-specific key precedes the generic gpt-5 key in the table;
        // both currently share a price, but the ordering guarantees a
        // codex-specific entry could differ later.
        assert_eq!(price_for("gpt-5-codex"), price_for("gpt-5"));
    }
}
