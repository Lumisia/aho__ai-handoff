//! `ai-handoff config get|set|list` — view and edit the shared config.
//!
//! All three read/write the single `~/.ai-handoff/config.toml` that the daemon
//! (and therefore both Claude and Codex) already consume, so an edit applies to
//! both agents at once. Writes are never-clobber (see `core::config::set_value`)
//! and land via a same-directory temp file + rename so a crash mid-write can
//! never leave a half-written config.

use std::io::Write;
use std::path::Path;

use ai_handoff_core::config;

use crate::ConfigAction;

pub fn run(action: ConfigAction) -> anyhow::Result<i32> {
    let path = ai_handoff_core::paths::config_path();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    run_io(action, &path, &mut out)
}

pub fn run_io(action: ConfigAction, path: &Path, out: &mut dyn Write) -> anyhow::Result<i32> {
    match action {
        ConfigAction::Get { key } => {
            let cfg = config::load_from(path);
            match config::get_value(&cfg, &key) {
                Ok(v) => {
                    writeln!(out, "{v}")?;
                    Ok(0)
                }
                Err(e) => {
                    writeln!(out, "error: {e}")?;
                    Ok(2)
                }
            }
        }
        ConfigAction::List => {
            let cfg = config::load_from(path);
            for key in config::settable_keys() {
                let v = config::get_value(&cfg, key).unwrap_or_default();
                writeln!(out, "{key} = {v}")?;
            }
            Ok(0)
        }
        ConfigAction::Set { key, value } => {
            let existing = std::fs::read_to_string(path).ok();
            match config::set_value(existing.as_deref(), &key, &value) {
                Ok(text) => {
                    write_atomic(path, &text)?;
                    writeln!(out, "{key} = {value}")?;
                    Ok(0)
                }
                Err(e) => {
                    writeln!(out, "error: {e}")?;
                    Ok(2)
                }
            }
        }
    }
}

/// Write `text` to `path` atomically: create the parent dir, write a sibling
/// temp file, then rename it over the target.
fn write_atomic(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConfigAction;

    fn run_get(path: &Path, key: &str) -> (i32, String) {
        let mut out = Vec::new();
        let code = run_io(ConfigAction::Get { key: key.into() }, path, &mut out).unwrap();
        (code, String::from_utf8(out).unwrap())
    }

    fn run_set(path: &Path, key: &str, value: &str) -> (i32, String) {
        let mut out = Vec::new();
        let code = run_io(
            ConfigAction::Set {
                key: key.into(),
                value: value.into(),
            },
            path,
            &mut out,
        )
        .unwrap();
        (code, String::from_utf8(out).unwrap())
    }

    #[test]
    fn set_then_get_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let (code, _) = run_set(&path, "triggers.five_hour.threshold_percent", "65");
        assert_eq!(code, 0);
        assert!(path.exists());

        let (code, text) = run_get(&path, "triggers.five_hour.threshold_percent");
        assert_eq!(code, 0);
        assert_eq!(text.trim(), "65");
    }

    #[test]
    fn get_on_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (code, text) = run_get(&path, "statusline.show");
        assert_eq!(code, 0);
        assert_eq!(text.trim(), "true");
    }

    #[test]
    fn set_unknown_key_exits_two_and_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (code, text) = run_set(&path, "triggers.five_hour.bogus", "1");
        assert_eq!(code, 2);
        assert!(text.contains("unknown config key"));
        assert!(!path.exists(), "rejected set must not create a file");
    }

    #[test]
    fn set_invalid_value_exits_two_without_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[statusline]\nshow = false\n# mine\n").unwrap();

        let (code, text) = run_set(&path, "triggers.five_hour.threshold_percent", "999");
        assert_eq!(code, 2);
        assert!(text.contains("invalid value"));
        // original file is untouched
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("# mine"));
        assert!(on_disk.contains("show = false"));
    }

    #[test]
    fn list_prints_all_editable_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut out = Vec::new();
        let code = run_io(ConfigAction::List, &path, &mut out).unwrap();
        assert_eq!(code, 0);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), config::settable_keys().count());
        assert!(text.contains("statusline.show = true"));
        assert!(text.contains("triggers.five_hour.mode = ask"));
        assert!(text.contains("language = en"));
        assert!(text.contains("capsule.remaining_max_items = 5"));
    }

    #[test]
    fn set_preserves_foreign_lines_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# keep\n[autostart]\nenabled = true\n").unwrap();

        let (code, _) = run_set(&path, "statusline.show", "false");
        assert_eq!(code, 0);
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("# keep"));
        assert!(on_disk.contains("[autostart]"));
        // and the new value loads
        assert!(!config::load_from(&path).statusline.show);
        assert!(config::load_from(&path).autostart.enabled);
    }
}
