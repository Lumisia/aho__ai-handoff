//! Typed, file-backed AI Handoff config (`~/.ai-handoff/config.toml`).
//!
//! Embedded defaults match v1's `defaults.json`. `parse`/`resolve` are pure;
//! `load` is the only IO entry point. A missing or malformed config resolves to
//! defaults so a hook is never broken by a bad config.

use std::collections::HashMap;
use std::path::Path;

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
    pub fn to_burn(self) -> BurnRate {
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

/// Resolve the effective trigger for a project: apply the matching
/// `project_overrides[fingerprint]` (if any) over the global config, then clamp.
pub fn resolve(cfg: &Config, fingerprint: &str) -> ResolvedTrigger {
    let g = &cfg.triggers.five_hour;
    let mut enabled = g.enabled;
    let mut threshold = g.threshold_percent;
    let mut mode = g.mode;
    let mut burn = g.burn_rate;

    if let Some(ov) = cfg.project_overrides.get(fingerprint) {
        let f = &ov.triggers.five_hour;
        if let Some(v) = f.enabled {
            enabled = v;
        }
        if let Some(v) = f.threshold_percent {
            threshold = v;
        }
        if let Some(v) = f.mode {
            mode = v;
        }
        if let Some(b) = f.burn_rate.as_ref() {
            if let Some(v) = b.enabled {
                burn.enabled = v;
            }
            if let Some(v) = b.runway_minutes {
                burn.runway_minutes = v;
            }
        }
    }

    ResolvedTrigger {
        enabled,
        threshold: threshold.clamp(0.0, 100.0),
        mode: mode.to_trigger_mode(),
        burn: burn.to_burn(),
    }
}

/// Load config from the default path, falling back to defaults on any error.
pub fn load() -> Config {
    load_from(&crate::paths::config_path())
}

/// Load config from `path`. A missing file or any parse error yields
/// `Config::default()` — the daemon must never break a hook over a bad config.
pub fn load_from(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).unwrap_or_default(),
        Err(_) => Config::default(),
    }
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

    #[test]
    fn resolve_without_override_uses_global() {
        let c = parse("[triggers.five_hour]\nthreshold_percent = 80\nmode = \"ask\"\n").unwrap();
        let r = resolve(&c, "fp-none");
        assert!(r.enabled);
        assert_eq!(r.threshold, 80.0);
        assert_eq!(r.mode, TriggerMode::Ask);
        assert!(!r.burn.enabled);
    }

    #[test]
    fn resolve_applies_matching_override_and_inherits_rest() {
        let c = parse(
            "[triggers.five_hour]\n\
             threshold_percent = 80\n\
             mode = \"ask\"\n\
             [project_overrides.\"fpX\".triggers.five_hour]\n\
             threshold_percent = 90\n\
             mode = \"auto\"\n",
        )
        .unwrap();
        let rx = resolve(&c, "fpX");
        assert_eq!(rx.threshold, 90.0);
        assert_eq!(rx.mode, TriggerMode::Auto);
        // a different project still gets the global values
        let ry = resolve(&c, "fpY");
        assert_eq!(ry.threshold, 80.0);
        assert_eq!(ry.mode, TriggerMode::Ask);
    }

    #[test]
    fn resolve_override_with_only_threshold_keeps_global_mode() {
        let c = parse(
            "[triggers.five_hour]\nmode = \"auto\"\n\
             [project_overrides.\"fpX\".triggers.five_hour]\nthreshold_percent = 50\n",
        )
        .unwrap();
        let r = resolve(&c, "fpX");
        assert_eq!(r.threshold, 50.0);
        assert_eq!(r.mode, TriggerMode::Auto); // inherited
    }

    #[test]
    fn resolve_clamps_threshold_into_0_100() {
        let hi = parse("[triggers.five_hour]\nthreshold_percent = 150\n").unwrap();
        assert_eq!(resolve(&hi, "x").threshold, 100.0);
        let lo = parse("[triggers.five_hour]\nthreshold_percent = -5\n").unwrap();
        assert_eq!(resolve(&lo, "x").threshold, 0.0);
    }

    #[test]
    fn load_from_missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let c = load_from(&dir.path().join("config.toml"));
        assert_eq!(c, Config::default());
        assert_eq!(c.triggers.five_hour.threshold_percent, 80.0);
    }

    #[test]
    fn load_from_malformed_file_is_default_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "this = = not valid toml").unwrap();
        assert_eq!(load_from(&p), Config::default());
    }

    #[test]
    fn load_from_valid_file_parses() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[triggers.five_hour]\nthreshold_percent = 42\n").unwrap();
        assert_eq!(load_from(&p).triggers.five_hour.threshold_percent, 42.0);
    }
}
