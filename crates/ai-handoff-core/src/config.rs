//! Typed, file-backed AI Handoff config (`~/.ai-handoff/config.toml`).
//!
//! Embedded defaults match v1's `defaults.json`. `parse`/`resolve` are pure;
//! `load` is the only IO entry point. A missing or malformed config resolves to
//! defaults so a hook is never broken by a bad config.

use std::collections::HashMap;

use serde::Deserialize;

use crate::trigger::{BurnRate, TriggerMode};

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub triggers: Triggers,
    pub project_overrides: HashMap<String, ProjectOverride>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Triggers {
    pub five_hour: FiveHour,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default)]
pub struct FiveHour {
    pub enabled: bool,
    pub threshold_percent: f64,
    pub mode: ModeCfg,
    pub burn_rate: BurnRateCfg,
}

impl Default for FiveHour {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_percent: 80.0,
            mode: ModeCfg::Ask,
            burn_rate: BurnRateCfg::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeCfg {
    Off,
    Ask,
    Auto,
}

impl ModeCfg {
    pub fn to_trigger_mode(self) -> TriggerMode {
        match self {
            ModeCfg::Off => TriggerMode::Off,
            ModeCfg::Ask => TriggerMode::Ask,
            ModeCfg::Auto => TriggerMode::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default)]
pub struct BurnRateCfg {
    pub enabled: bool,
    pub runway_minutes: f64,
}

impl Default for BurnRateCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            runway_minutes: 30.0,
        }
    }
}

impl BurnRateCfg {
    fn to_burn(self) -> BurnRate {
        BurnRate {
            enabled: self.enabled,
            runway_minutes: self.runway_minutes,
        }
    }
}

/// A per-project override: every field optional, deep-merged over the global.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ProjectOverride {
    pub triggers: TriggersOverride,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct TriggersOverride {
    pub five_hour: FiveHourOverride,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct FiveHourOverride {
    pub enabled: Option<bool>,
    pub threshold_percent: Option<f64>,
    pub mode: Option<ModeCfg>,
    pub burn_rate: Option<BurnRateOverride>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct BurnRateOverride {
    pub enabled: Option<bool>,
    pub runway_minutes: Option<f64>,
}

/// The concrete trigger inputs after applying any project override.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedTrigger {
    pub enabled: bool,
    pub threshold: f64,
    pub mode: TriggerMode,
    pub burn: BurnRate,
}

/// Parse config text. Propagates TOML/type errors so `load` can fall back.
pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_all_defaults() {
        let c = parse("").unwrap();
        let f = c.triggers.five_hour;
        assert!(f.enabled);
        assert_eq!(f.threshold_percent, 80.0);
        assert_eq!(f.mode, ModeCfg::Ask);
        assert!(!f.burn_rate.enabled);
        assert_eq!(f.burn_rate.runway_minutes, 30.0);
        assert!(c.project_overrides.is_empty());
    }

    #[test]
    fn parses_full_global_config() {
        let c = parse(
            "[triggers.five_hour]\n\
             enabled = true\n\
             threshold_percent = 70\n\
             mode = \"auto\"\n\
             [triggers.five_hour.burn_rate]\n\
             enabled = true\n\
             runway_minutes = 15\n",
        )
        .unwrap();
        let f = c.triggers.five_hour;
        assert_eq!(f.threshold_percent, 70.0);
        assert_eq!(f.mode, ModeCfg::Auto);
        assert!(f.burn_rate.enabled);
        assert_eq!(f.burn_rate.runway_minutes, 15.0);
    }

    #[test]
    fn mode_parses_each_lowercase_variant() {
        for (text, want) in [("off", ModeCfg::Off), ("ask", ModeCfg::Ask), ("auto", ModeCfg::Auto)] {
            let c = parse(&format!("[triggers.five_hour]\nmode = \"{text}\"\n")).unwrap();
            assert_eq!(c.triggers.five_hour.mode, want);
        }
    }

    #[test]
    fn unknown_mode_is_parse_error() {
        assert!(parse("[triggers.five_hour]\nmode = \"weird\"\n").is_err());
    }

    #[test]
    fn partial_section_keeps_other_defaults() {
        // only threshold given; mode/enabled/burn stay default
        let c = parse("[triggers.five_hour]\nthreshold_percent = 55\n").unwrap();
        let f = c.triggers.five_hour;
        assert_eq!(f.threshold_percent, 55.0);
        assert_eq!(f.mode, ModeCfg::Ask);
        assert!(f.enabled);
    }
}
