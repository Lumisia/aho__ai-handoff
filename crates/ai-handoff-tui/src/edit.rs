//! Pure config-edit logic for the Settings tab: compute the next value for a
//! key given a keypress, then commit it to disk via the core never-clobber
//! writer. No ratatui here so the value transitions are unit-testable.

use std::path::Path;

use ai_handoff_core::config::{self, ConfigWriteError, KeyKind};

/// A Settings-tab edit gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    /// Space — flip a bool / advance an enum / step a number up.
    Toggle,
    /// Right / `+` — next enum value or step a number up.
    Next,
    /// Left / `-` — previous enum value or step a number down.
    Prev,
}

const PERCENT_STEP: f64 = 5.0;
const MINUTES_STEP: f64 = 5.0;
const COUNT_STEP: i64 = 1;
const SECONDS_STEP: i64 = 5;
const MODES: [&str; 3] = ["off", "ask", "auto"];
const LANGS: [&str; 3] = ["en", "ko", "ja"];
const CAPSULE_FORMATS: [&str; 2] = ["json", "md"];
const THEME_PRESETS: [&str; 4] = ["default", "high_contrast", "mono", "custom"];
const GUI_THEME_PRESETS: [&str; 3] = ["white", "dark", "custom"];
const COLOR_PRESETS: [&str; 18] = [
    "#B996EB",
    "#E68C1E",
    "#FFA500",
    "cyan",
    "light-cyan",
    "light-blue",
    "blue",
    "magenta",
    "purple",
    "red",
    "green",
    "dark-gray",
    "black",
    "white",
    "gray",
    "#005F87",
    "#5F005F",
    "#005F00",
];

/// Compute the next raw value for `key`'s `kind`, given the current effective
/// `current` value and an `action`. Returns `None` if `current` is unparseable
/// (the caller keeps the old value).
pub fn next_raw(kind: KeyKind, current: &str, action: EditAction) -> Option<String> {
    match kind {
        KeyKind::Bool => {
            let now: bool = current.parse().ok()?;
            Some((!now).to_string())
        }
        KeyKind::Mode => Some(cycle(&MODES, current, action)),
        KeyKind::Lang => Some(cycle(&LANGS, current, action)),
        KeyKind::CapsuleFormat => Some(cycle(&CAPSULE_FORMATS, current, action)),
        KeyKind::ThemePreset => Some(cycle(&THEME_PRESETS, current, action)),
        KeyKind::GuiThemePreset => Some(cycle(&GUI_THEME_PRESETS, current, action)),
        KeyKind::Color => Some(cycle(&COLOR_PRESETS, current, action)),
        KeyKind::Percent => {
            let now: f64 = current.parse().ok()?;
            let delta = if action == EditAction::Prev {
                -PERCENT_STEP
            } else {
                PERCENT_STEP
            };
            Some(fmt_num((now + delta).clamp(0.0, 100.0)))
        }
        KeyKind::PosFloat => {
            let now: f64 = current.parse().ok()?;
            let delta = if action == EditAction::Prev {
                -MINUTES_STEP
            } else {
                MINUTES_STEP
            };
            Some(fmt_num((now + delta).max(MINUTES_STEP)))
        }
        KeyKind::Count => {
            let now: i64 = current.parse().ok()?;
            let delta = if action == EditAction::Prev {
                -COUNT_STEP
            } else {
                COUNT_STEP
            };
            Some((now + delta).clamp(1, 50).to_string())
        }
        KeyKind::Seconds => {
            let now: i64 = current.parse().ok()?;
            let delta = if action == EditAction::Prev {
                -SECONDS_STEP
            } else {
                SECONDS_STEP
            };
            Some((now + delta).clamp(1, 3600).to_string())
        }
    }
}

/// Cycle through a fixed list of string values (wrapping), stepping back on
/// `Prev` and forward otherwise. Unknown `current` starts at index 0.
fn cycle(values: &[&str], current: &str, action: EditAction) -> String {
    let idx = values.iter().position(|v| *v == current).unwrap_or(0);
    let len = values.len();
    let next = match action {
        EditAction::Prev => (idx + len - 1) % len,
        _ => (idx + 1) % len,
    };
    values[next].to_string()
}

/// Error from committing a settings edit.
#[derive(Debug)]
pub enum CommitError {
    Validation(ConfigWriteError),
    Io(std::io::Error),
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommitError::Validation(e) => write!(f, "{e}"),
            CommitError::Io(e) => write!(f, "write failed: {e}"),
        }
    }
}

/// Read `path`, set `key=raw` (never-clobber), and atomically write it back.
/// Returns the new effective value string on success.
pub fn commit(path: &Path, key: &str, raw: &str) -> Result<String, CommitError> {
    let existing = std::fs::read_to_string(path).ok();
    let text = config::set_value(existing.as_deref(), key, raw).map_err(CommitError::Validation)?;
    write_atomic(path, &text).map_err(CommitError::Io)?;
    Ok(raw.to_string())
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Format a float without a redundant `.0` tail (mirrors core's display).
fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_toggles() {
        assert_eq!(
            next_raw(KeyKind::Bool, "true", EditAction::Toggle).unwrap(),
            "false"
        );
        assert_eq!(
            next_raw(KeyKind::Bool, "false", EditAction::Next).unwrap(),
            "true"
        );
    }

    #[test]
    fn mode_cycles_both_directions() {
        assert_eq!(
            next_raw(KeyKind::Mode, "off", EditAction::Next).unwrap(),
            "ask"
        );
        assert_eq!(
            next_raw(KeyKind::Mode, "ask", EditAction::Next).unwrap(),
            "auto"
        );
        assert_eq!(
            next_raw(KeyKind::Mode, "auto", EditAction::Next).unwrap(),
            "off"
        );
        assert_eq!(
            next_raw(KeyKind::Mode, "off", EditAction::Prev).unwrap(),
            "auto"
        );
    }

    #[test]
    fn lang_cycles_through_three_codes() {
        assert_eq!(
            next_raw(KeyKind::Lang, "en", EditAction::Next).unwrap(),
            "ko"
        );
        assert_eq!(
            next_raw(KeyKind::Lang, "ko", EditAction::Next).unwrap(),
            "ja"
        );
        assert_eq!(
            next_raw(KeyKind::Lang, "ja", EditAction::Next).unwrap(),
            "en"
        );
        assert_eq!(
            next_raw(KeyKind::Lang, "en", EditAction::Prev).unwrap(),
            "ja"
        );
    }

    #[test]
    fn capsule_format_cycles() {
        assert_eq!(
            next_raw(KeyKind::CapsuleFormat, "json", EditAction::Next).unwrap(),
            "md"
        );
        assert_eq!(
            next_raw(KeyKind::CapsuleFormat, "md", EditAction::Next).unwrap(),
            "json"
        );
    }

    #[test]
    fn theme_preset_and_colors_cycle() {
        assert_eq!(
            next_raw(KeyKind::ThemePreset, "default", EditAction::Next).unwrap(),
            "high_contrast"
        );
        assert_eq!(
            next_raw(KeyKind::Color, "#B996EB", EditAction::Next).unwrap(),
            "#E68C1E"
        );
        assert_eq!(
            next_raw(KeyKind::Color, "blue", EditAction::Next).unwrap(),
            "magenta"
        );
        assert_eq!(
            next_raw(KeyKind::Color, "blue", EditAction::Prev).unwrap(),
            "light-blue"
        );
    }

    #[test]
    fn percent_steps_and_clamps() {
        assert_eq!(
            next_raw(KeyKind::Percent, "80", EditAction::Next).unwrap(),
            "85"
        );
        assert_eq!(
            next_raw(KeyKind::Percent, "98", EditAction::Next).unwrap(),
            "100"
        );
        assert_eq!(
            next_raw(KeyKind::Percent, "2", EditAction::Prev).unwrap(),
            "0"
        );
    }

    #[test]
    fn posfloat_steps_with_floor() {
        assert_eq!(
            next_raw(KeyKind::PosFloat, "30", EditAction::Next).unwrap(),
            "35"
        );
        assert_eq!(
            next_raw(KeyKind::PosFloat, "5", EditAction::Prev).unwrap(),
            "5"
        );
    }

    #[test]
    fn count_steps_and_clamps() {
        assert_eq!(
            next_raw(KeyKind::Count, "5", EditAction::Next).unwrap(),
            "6"
        );
        assert_eq!(
            next_raw(KeyKind::Count, "1", EditAction::Prev).unwrap(),
            "1"
        );
    }

    #[test]
    fn seconds_step_and_clamp_to_daemon_range() {
        assert_eq!(
            next_raw(KeyKind::Seconds, "60", EditAction::Next).unwrap(),
            "65"
        );
        assert_eq!(
            next_raw(KeyKind::Seconds, "1", EditAction::Prev).unwrap(),
            "1"
        );
        assert_eq!(
            next_raw(KeyKind::Seconds, "3600", EditAction::Next).unwrap(),
            "3600"
        );
    }

    #[test]
    fn unparseable_current_yields_none() {
        assert!(next_raw(KeyKind::Percent, "abc", EditAction::Next).is_none());
        assert!(next_raw(KeyKind::Bool, "maybe", EditAction::Toggle).is_none());
    }

    #[test]
    fn commit_writes_value_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        commit(&path, "triggers.five_hour.mode", "auto").unwrap();
        let cfg = config::load_from(&path);
        assert_eq!(
            cfg.triggers.five_hour.mode,
            ai_handoff_core::config::ModeCfg::Auto
        );
    }

    #[test]
    fn commit_preserves_foreign_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# keep\n[autostart]\nenabled = true\n").unwrap();
        commit(&path, "statusline.show", "false").unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("# keep"));
        assert!(on_disk.contains("[autostart]"));
        assert!(!config::load_from(&path).statusline.show);
    }

    #[test]
    fn commit_rejects_invalid_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let err = commit(&path, "triggers.five_hour.threshold_percent", "999").unwrap_err();
        assert!(matches!(err, CommitError::Validation(_)));
        assert!(!path.exists());
    }
}
