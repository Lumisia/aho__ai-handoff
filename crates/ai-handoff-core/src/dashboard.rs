use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    capsule::Capsule,
    capsule_codec,
    fingerprint::fingerprint,
    install::{duplicate, state},
    paths,
};

const EVENTS: [&str; 4] = ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Warning,
    Error,
    Missing,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CheckRow {
    pub id: String,
    pub label: String,
    pub status: CheckStatus,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DashboardPaths {
    pub ai_home: String,
    pub ipc: String,
    pub store: String,
    pub logs: String,
    pub install_state: String,
    pub codex_hooks: String,
    pub codex_config: String,
    pub claude_settings: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct InstallSummary {
    pub status: CheckStatus,
    pub version: u32,
    pub installed_at: String,
    pub autostart: String,
    pub launcher: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CapsuleSummary {
    pub capsule_id: String,
    pub project_id: String,
    pub project_label: String,
    pub created_at: String,
    pub source_agent: String,
    pub target_agent: String,
    pub state: String,
    pub summary_preview: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CapsuleList {
    pub items: Vec<CapsuleSummary>,
    pub pending_count: usize,
    pub skipped: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ReadTextResult {
    pub path: String,
    pub text: String,
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LogFile {
    pub name: String,
    pub result: ReadTextResult,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DashboardSnapshot {
    pub paths: DashboardPaths,
    pub install_state: InstallSummary,
    pub daemon: CheckRow,
    pub autostart: CheckRow,
    pub codex_hooks: CheckRow,
    pub codex_config: CheckRow,
    pub claude_settings: CheckRow,
    pub ipc: CheckRow,
    pub store: CheckRow,
    pub duplicates: Vec<CheckRow>,
    pub capsules: CapsuleList,
    pub checks: Vec<CheckRow>,
}

pub fn dashboard_snapshot() -> DashboardSnapshot {
    let home = paths::home();
    let user_home = directories::BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(|| home.clone());
    dashboard_snapshot_for(&home, &user_home)
}

pub fn dashboard_snapshot_for(home: &Path, user_home: &Path) -> DashboardSnapshot {
    let p = dashboard_paths(home, user_home);
    let install_state = read_install_summary(home);
    let install_record = state::load(home);
    let codex_hooks_text = fs::read_to_string(&p.codex_hooks).ok();
    let codex_config_text = fs::read_to_string(&p.codex_config).ok();
    let claude_settings_text = fs::read_to_string(&p.claude_settings).ok();

    let codex_hooks = check_codex_hooks(
        Path::new(&p.codex_hooks),
        codex_hooks_text.as_deref(),
        codex_config_text.as_deref(),
        &install_record.codex.plugin,
    );
    let codex_config = check_codex_config(
        Path::new(&p.codex_config),
        codex_config_text.as_deref(),
        &p.ipc,
    );
    let claude_settings = check_claude_settings(
        Path::new(&p.claude_settings),
        claude_settings_text.as_deref(),
    );
    let ipc = check_dir("ipc", "IPC", Path::new(&p.ipc));
    let store = check_dir("store", "Store", Path::new(&p.store));
    let autostart = check_autostart(&install_state);
    let daemon = CheckRow {
        id: "daemon".into(),
        label: "Daemon".into(),
        status: CheckStatus::Unknown,
        message: "Runtime status API not implemented in this MVP".into(),
        path: None,
    };
    let duplicates = duplicate::detect(
        codex_config_text.as_deref(),
        codex_hooks_text.as_deref(),
        claude_settings_text.as_deref(),
        false,
    )
    .into_iter()
    .enumerate()
    .map(|(idx, finding)| CheckRow {
        id: format!("duplicate-{idx}"),
        label: format!("Duplicate {}", finding.agent),
        status: CheckStatus::Warning,
        message: finding.detail,
        path: None,
    })
    .collect::<Vec<_>>();
    let capsules = list_capsules_for(home);

    let mut checks = vec![
        codex_hooks.clone(),
        codex_config.clone(),
        claude_settings.clone(),
        ipc.clone(),
        store.clone(),
    ];
    checks.extend(duplicates.clone());

    DashboardSnapshot {
        paths: p,
        install_state,
        daemon,
        autostart,
        codex_hooks,
        codex_config,
        claude_settings,
        ipc,
        store,
        duplicates,
        capsules,
        checks,
    }
}

pub fn list_capsules() -> CapsuleList {
    list_capsules_for(&paths::home())
}

pub fn list_capsules_for(home: &Path) -> CapsuleList {
    let current = std::env::current_dir().ok();
    list_capsules_for_with_current(home, current.as_deref())
}

fn list_capsules_for_with_current(home: &Path, current_cwd: Option<&Path>) -> CapsuleList {
    let root = home.join("store").join("capsules");
    let mut items = Vec::new();
    let mut skipped = 0usize;

    if let Ok(projects) = fs::read_dir(root) {
        for project in projects.flatten() {
            if !project.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let project_id = project.file_name().to_string_lossy().into_owned();
            let project_label = project_label_for(&project.path(), &project_id, current_cwd);
            if let Ok(files) = fs::read_dir(project.path()) {
                for file in files.flatten() {
                    let path = file.path();
                    if !is_capsule_file(&path) {
                        continue;
                    }
                    match capsule_codec::read_capsule(&path) {
                        Ok(capsule) => {
                            items.push(summary_from_capsule(capsule, path, project_label.clone()))
                        }
                        Err(_) => skipped += 1,
                    }
                }
            }
        }
    }

    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let pending_count = items.iter().filter(|i| i.state == "pending").count();
    CapsuleList {
        items,
        pending_count,
        skipped,
    }
}

fn project_label_for(project_dir: &Path, project_id: &str, current_cwd: Option<&Path>) -> String {
    let from_sidecar = fs::read_to_string(project_dir.join("project.label"))
        .ok()
        .map(|label| label.trim().to_string())
        .filter(|label| !label.is_empty());
    if let Some(label) = from_sidecar {
        return label;
    }

    if let Some(cwd) = current_cwd {
        if fingerprint(cwd) == project_id {
            if let Some(label) = path_label(cwd) {
                return label;
            }
        }
    }

    project_id.to_string()
}

fn path_label(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn is_capsule_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("json" | "md")
    )
}

pub fn read_capsule(path: &Path, max_bytes: u64) -> ReadTextResult {
    read_text(path, max_bytes)
}

pub fn read_logs(max_bytes: u64) -> Vec<LogFile> {
    read_logs_for(&paths::home(), max_bytes)
}

/// List the log files that actually exist under `<home>/logs`, sorted by
/// name. Only real logs are shown: hardcoding names here used to surface
/// never-written files (`hook.log`, `install.log`) as os-error rows in the
/// UIs. Matching on `.log` also picks up rotated files (`daemon.log.1`).
pub fn read_logs_for(home: &Path, max_bytes: u64) -> Vec<LogFile> {
    let logs = home.join("logs");
    let mut names: Vec<String> = std::fs::read_dir(&logs)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_string();
            (path.is_file() && name.contains(".log")).then_some(name)
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| LogFile {
            result: read_log_newest_first(&logs.join(&name), max_bytes),
            name,
        })
        .collect()
}

/// Read an append-only log for display: newest line first, spending the
/// `max_bytes` budget on the NEWEST lines. `read_text`'s plain head slice
/// would keep a growing log's oldest lines and cut off exactly the part
/// being debugged.
fn read_log_newest_first(path: &Path, max_bytes: u64) -> ReadTextResult {
    let raw = read_text(path, u64::MAX);
    if raw.error.is_some() {
        return raw;
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut used: u64 = 0;
    let mut truncated = false;
    for line in raw.text.lines().rev() {
        used += line.len() as u64 + 1;
        // Always keep at least one line so a single oversized line still shows.
        if used > max_bytes && !kept.is_empty() {
            truncated = true;
            break;
        }
        kept.push(line);
    }
    ReadTextResult {
        path: raw.path,
        text: kept.join("\n"),
        truncated,
        error: None,
    }
}

fn dashboard_paths(home: &Path, user_home: &Path) -> DashboardPaths {
    DashboardPaths {
        ai_home: home.to_string_lossy().into_owned(),
        ipc: home.join("ipc").to_string_lossy().into_owned(),
        store: home.join("store").to_string_lossy().into_owned(),
        logs: home.join("logs").to_string_lossy().into_owned(),
        install_state: state::state_path(home).to_string_lossy().into_owned(),
        codex_hooks: user_home
            .join(".codex")
            .join("hooks.json")
            .to_string_lossy()
            .into_owned(),
        codex_config: user_home
            .join(".codex")
            .join("config.toml")
            .to_string_lossy()
            .into_owned(),
        claude_settings: user_home
            .join(".claude")
            .join("settings.json")
            .to_string_lossy()
            .into_owned(),
    }
}

fn read_install_summary(home: &Path) -> InstallSummary {
    let path = state::state_path(home);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return InstallSummary {
                status: CheckStatus::Missing,
                version: 1,
                installed_at: String::new(),
                autostart: "missing".into(),
                launcher: None,
            }
        }
    };

    match serde_json::from_slice::<state::InstallState>(&bytes) {
        Ok(st) => InstallSummary {
            status: CheckStatus::Ok,
            version: st.version,
            installed_at: st.installed_at,
            autostart: st
                .autostart
                .map(|a| format!("{:?}: {}", a.kind, a.name))
                .or(st
                    .scheduled_task
                    .map(|name| format!("ScheduledTask: {name}")))
                .unwrap_or_else(|| "missing".into()),
            launcher: st.launcher.and_then(|launcher| launcher.path),
        },
        Err(error) => InstallSummary {
            status: CheckStatus::Error,
            version: 1,
            installed_at: String::new(),
            autostart: format!("parse error: {error}"),
            launcher: None,
        },
    }
}

fn check_dir(id: &str, label: &str, path: &Path) -> CheckRow {
    match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => {
            let permissions = crate::secure_fs::private_dir_status(path);
            let (status, message) = match permissions.status {
                crate::secure_fs::PermissionStatus::Ok => {
                    (CheckStatus::Ok, format!("present; {}", permissions.message))
                }
                crate::secure_fs::PermissionStatus::Warning => (
                    CheckStatus::Warning,
                    format!("present but permissions are broad: {}", permissions.message),
                ),
                crate::secure_fs::PermissionStatus::Error => (
                    CheckStatus::Error,
                    format!(
                        "present but permissions could not be checked: {}",
                        permissions.message
                    ),
                ),
                crate::secure_fs::PermissionStatus::Missing => {
                    (CheckStatus::Missing, "missing".into())
                }
            };
            CheckRow {
                id: id.into(),
                label: label.into(),
                status,
                message,
                path: Some(path.to_string_lossy().into_owned()),
            }
        }
        Ok(_) => CheckRow {
            id: id.into(),
            label: label.into(),
            status: CheckStatus::Error,
            message: "path exists but is not a directory".into(),
            path: Some(path.to_string_lossy().into_owned()),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => CheckRow {
            id: id.into(),
            label: label.into(),
            status: CheckStatus::Missing,
            message: "missing".into(),
            path: Some(path.to_string_lossy().into_owned()),
        },
        Err(error) => CheckRow {
            id: id.into(),
            label: label.into(),
            status: CheckStatus::Error,
            message: error.to_string(),
            path: Some(path.to_string_lossy().into_owned()),
        },
    }
}

fn check_codex_hooks(
    path: &Path,
    existing: Option<&str>,
    codex_config: Option<&str>,
    plugin: &Option<state::PluginRecord>,
) -> CheckRow {
    if let Some(row) = check_plugin_hooks("codex-hooks", "Codex hooks", plugin) {
        if row.status != CheckStatus::Ok {
            return row;
        }
        let config_text = codex_config.unwrap_or_default();
        let enabled = duplicate::codex_v2_plugin_enabled(config_text);
        let trusted = duplicate::codex_v2_plugin_trusted(config_text);
        if enabled && trusted {
            return row;
        }
        return CheckRow {
            id: "codex-hooks".into(),
            label: "Codex hooks".into(),
            status: CheckStatus::Warning,
            message: if enabled {
                "v2 plugin hooks need trust in Codex /hooks".into()
            } else {
                "v2 plugin disabled in Codex config".into()
            },
            path: row.path,
        };
    }

    let Some(text) = existing else {
        return missing_row("codex-hooks", "Codex hooks", path);
    };
    let value = match serde_json::from_str::<Value>(text) {
        Ok(value) => value,
        Err(error) => {
            return error_row(
                "codex-hooks",
                "Codex hooks",
                path,
                format!("parse error: {error}"),
            )
        }
    };
    let hooks = value.get("hooks").and_then(Value::as_object);
    let installed = hooks
        .map(|obj| {
            EVENTS
                .iter()
                .all(|event| event_has_ai_handoff(obj.get(*event)))
        })
        .unwrap_or(false);
    if installed {
        ok_row("codex-hooks", "Codex hooks", path, "v2 hooks installed")
    } else {
        CheckRow {
            id: "codex-hooks".into(),
            label: "Codex hooks".into(),
            status: CheckStatus::Warning,
            message: "v2 hooks missing or incomplete".into(),
            path: Some(path.to_string_lossy().into_owned()),
        }
    }
}

fn check_claude_settings(path: &Path, existing: Option<&str>) -> CheckRow {
    // Claude Code loads hooks from ~/.claude/settings.json, not from the plugin
    // bundle: the Claude plugin ships skills only and intentionally never writes
    // a hooks/hooks.json (see install::plugin). Validate settings.json directly.
    // Delegating to check_plugin_hooks here reported a false "missing" because
    // the Claude plugin root has no hooks.json to find.
    let Some(text) = existing else {
        return missing_row("claude-settings", "Claude settings", path);
    };
    let value = match serde_json::from_str::<Value>(text) {
        Ok(value) => value,
        Err(error) => {
            return error_row(
                "claude-settings",
                "Claude settings",
                path,
                format!("parse error: {error}"),
            )
        }
    };
    let hooks = value.get("hooks").and_then(Value::as_object);
    let installed = hooks
        .map(|obj| {
            EVENTS
                .iter()
                .all(|event| event_has_ai_handoff(obj.get(*event)))
        })
        .unwrap_or(false);
    if installed {
        ok_row(
            "claude-settings",
            "Claude settings",
            path,
            "v2 hooks installed",
        )
    } else {
        CheckRow {
            id: "claude-settings".into(),
            label: "Claude settings".into(),
            status: CheckStatus::Warning,
            message: "v2 hooks missing or incomplete".into(),
            path: Some(path.to_string_lossy().into_owned()),
        }
    }
}

fn check_codex_config(path: &Path, existing: Option<&str>, ipc: &str) -> CheckRow {
    let Some(text) = existing else {
        return missing_row("codex-config", "Codex config", path);
    };
    let doc = match text.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        Err(error) => {
            return error_row(
                "codex-config",
                "Codex config",
                path,
                format!("parse error: {error}"),
            )
        }
    };
    let has_root = doc
        .get("sandbox_workspace_write")
        .and_then(|t| t.as_table())
        .and_then(|t| t.get("writable_roots"))
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().any(|v| v.as_str() == Some(ipc)))
        .unwrap_or(false);
    let has_home = doc
        .get("shell_environment_policy")
        .and_then(|t| t.as_table())
        .and_then(|t| t.get("set"))
        .and_then(|s| s.as_table())
        .and_then(|set| set.get("AI_HANDOFF_HOME"))
        .and_then(|v| v.as_str())
        .is_some();
    if has_root && has_home {
        ok_row(
            "codex-config",
            "Codex config",
            path,
            "writable_roots and AI_HANDOFF_HOME present",
        )
    } else {
        CheckRow {
            id: "codex-config".into(),
            label: "Codex config".into(),
            status: CheckStatus::Warning,
            message: format!("writable_roots={has_root}, AI_HANDOFF_HOME={has_home}"),
            path: Some(path.to_string_lossy().into_owned()),
        }
    }
}

fn check_plugin_hooks(
    id: &str,
    label: &str,
    plugin: &Option<state::PluginRecord>,
) -> Option<CheckRow> {
    let rec = plugin.as_ref()?;
    let path = Path::new(&rec.root).join("hooks").join("hooks.json");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Some(missing_row(id, label, &path));
        }
        Err(error) => return Some(error_row(id, label, &path, error.to_string())),
    };
    let value = match serde_json::from_str::<Value>(&text) {
        Ok(value) => value,
        Err(error) => return Some(error_row(id, label, &path, format!("parse error: {error}"))),
    };
    let hooks = value.get("hooks").and_then(Value::as_object);
    let installed = hooks
        .map(|obj| {
            EVENTS
                .iter()
                .all(|event| event_has_ai_handoff(obj.get(*event)))
        })
        .unwrap_or(false);
    if installed {
        Some(ok_row(id, label, &path, "v2 hooks installed"))
    } else {
        Some(CheckRow {
            id: id.into(),
            label: label.into(),
            status: CheckStatus::Warning,
            message: "v2 hooks missing or incomplete".into(),
            path: Some(path.to_string_lossy().into_owned()),
        })
    }
}

fn event_has_ai_handoff(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_array)
        .map(|outer| {
            outer.iter().any(|entry| {
                entry
                    .get("hooks")
                    .and_then(Value::as_array)
                    .map(|inner| {
                        inner.iter().any(|hook| {
                            hook.get("_aiHandoff").and_then(Value::as_bool) == Some(true)
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn summary_from_capsule(capsule: Capsule, path: PathBuf, project_label: String) -> CapsuleSummary {
    CapsuleSummary {
        capsule_id: capsule.capsule_id,
        project_id: capsule.project_id,
        project_label,
        created_at: capsule.created_at,
        source_agent: capsule.source_agent,
        target_agent: capsule.target_agent.unwrap_or_else(|| "any".into()),
        state: capsule.consumption.state.as_str().into(),
        summary_preview: capsule.summary.goal,
        path: path.to_string_lossy().into_owned(),
    }
}

fn read_text(path: &Path, max_bytes: u64) -> ReadTextResult {
    match fs::read(path) {
        Ok(bytes) => {
            let truncated = bytes.len() as u64 > max_bytes;
            let slice_len = bytes.len().min(max_bytes as usize);
            ReadTextResult {
                path: path.to_string_lossy().into_owned(),
                text: String::from_utf8_lossy(&bytes[..slice_len]).into_owned(),
                truncated,
                error: None,
            }
        }
        Err(error) => ReadTextResult {
            path: path.to_string_lossy().into_owned(),
            text: String::new(),
            truncated: false,
            error: Some(error.to_string()),
        },
    }
}

fn check_autostart(install: &InstallSummary) -> CheckRow {
    let status = if install.autostart == "missing" {
        CheckStatus::Warning
    } else {
        CheckStatus::Ok
    };
    CheckRow {
        id: "autostart".into(),
        label: "Autostart".into(),
        status,
        message: install.autostart.clone(),
        path: install.launcher.clone(),
    }
}

fn ok_row(id: &str, label: &str, path: &Path, message: &str) -> CheckRow {
    CheckRow {
        id: id.into(),
        label: label.into(),
        status: CheckStatus::Ok,
        message: message.into(),
        path: Some(path.to_string_lossy().into_owned()),
    }
}

fn missing_row(id: &str, label: &str, path: &Path) -> CheckRow {
    CheckRow {
        id: id.into(),
        label: label.into(),
        status: CheckStatus::Missing,
        message: "missing".into(),
        path: Some(path.to_string_lossy().into_owned()),
    }
}

fn error_row(id: &str, label: &str, path: &Path, message: String) -> CheckRow {
    CheckRow {
        id: id.into(),
        label: label.into(),
        status: CheckStatus::Error,
        message,
        path: Some(path.to_string_lossy().into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capsule::{Capsule, Consumption, ConsumptionState, RedactionMeta, Session, Summary};

    fn write(path: &std::path::Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn capsule(id: &str, project_id: &str, created_at: &str) -> Capsule {
        Capsule {
            schema_version: 2,
            capsule_id: id.into(),
            project_id: project_id.into(),
            created_at: created_at.into(),
            source_agent: "claude-code".into(),
            target_agent: Some("codex".into()),
            session: Session::default(),
            summary: Summary {
                goal: "ship dashboard".into(),
                done: vec!["core model".into()],
                remaining: vec!["ui".into()],
                risks: vec![],
            },
            files: vec![],
            next_prompt: Some("continue".into()),
            workspace: None,
            redaction: RedactionMeta {
                applied: false,
                ruleset: "none".into(),
            },
            consumption: Consumption {
                state: ConsumptionState::Pending,
                consumed_by: None,
                consumed_at: None,
                consumed_despite_target: false,
            },
        }
    }

    #[test]
    fn read_logs_lists_only_existing_log_files_sorted() {
        let temp = tempfile::tempdir().unwrap();
        write(&temp.path().join("logs").join("daemon.log.1"), "old");
        write(
            &temp.path().join("logs").join("daemon.log"),
            "10:00 first\n10:05 second\n",
        );
        // Non-log files in the directory never show up as log rows.
        write(&temp.path().join("logs").join("notes.txt"), "ignore");

        let logs = read_logs_for(temp.path(), 1024);
        let names = logs.iter().map(|log| log.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names, ["daemon.log", "daemon.log.1"]);
        assert!(logs.iter().all(|log| log.result.error.is_none()));
        // Newest line first for display.
        assert_eq!(logs[0].result.text, "10:05 second\n10:00 first");
    }

    #[test]
    fn read_log_budget_is_spent_on_newest_lines() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("logs").join("daemon.log");
        write(&path, "aaaa old old old\nbb newest\n");

        let result = read_log_newest_first(&path, 12);
        assert_eq!(result.text, "bb newest");
        assert!(
            result.truncated,
            "older lines beyond the budget are dropped"
        );
    }

    #[test]
    fn read_logs_is_empty_when_no_logs_were_written_yet() {
        let temp = tempfile::tempdir().unwrap();
        // No logs dir at all (fresh install): no rows, no os-error entries —
        // the old hardcoded list surfaced never-written files as errors.
        assert!(read_logs_for(temp.path(), 1024).is_empty());

        std::fs::create_dir_all(temp.path().join("logs")).unwrap();
        assert!(read_logs_for(temp.path(), 1024).is_empty());
    }

    #[test]
    fn missing_files_are_reported_not_fatal() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = dashboard_snapshot_for(temp.path(), temp.path());

        assert_eq!(
            snapshot.paths.ai_home,
            temp.path().to_string_lossy().into_owned()
        );
        assert_eq!(snapshot.install_state.status, CheckStatus::Missing);
        assert_eq!(snapshot.codex_hooks.status, CheckStatus::Missing);
        assert_eq!(snapshot.codex_config.status, CheckStatus::Missing);
        assert_eq!(snapshot.claude_settings.status, CheckStatus::Missing);
        assert_eq!(snapshot.ipc.status, CheckStatus::Missing);
        assert_eq!(snapshot.store.status, CheckStatus::Missing);
    }

    #[test]
    fn dashboard_checks_hide_runtime_daemon_and_autostart_rows() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = dashboard_snapshot_for(temp.path(), temp.path());
        let ids = snapshot
            .checks
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>();

        assert!(!ids.contains(&"daemon"));
        assert!(!ids.contains(&"autostart"));
        assert!(ids.contains(&"codex-hooks"));
        assert!(ids.contains(&"codex-config"));
        assert!(ids.contains(&"claude-settings"));
        assert!(ids.contains(&"ipc"));
        assert!(ids.contains(&"store"));
    }

    #[test]
    fn malformed_configs_are_parse_errors_without_panic() {
        let temp = tempfile::tempdir().unwrap();
        write(&temp.path().join(".codex/hooks.json"), "{bad json");
        write(&temp.path().join(".codex/config.toml"), "bad = = toml");
        write(&temp.path().join(".claude/settings.json"), "{bad json");

        let ai_home = temp.path().join(".ai-handoff");
        let snapshot = dashboard_snapshot_for(&ai_home, temp.path());

        assert_eq!(snapshot.codex_hooks.status, CheckStatus::Error);
        assert!(snapshot.codex_hooks.message.contains("parse"));
        assert_eq!(snapshot.codex_config.status, CheckStatus::Error);
        assert!(snapshot.codex_config.message.contains("parse"));
        assert_eq!(snapshot.claude_settings.status, CheckStatus::Error);
        assert!(snapshot.claude_settings.message.contains("parse"));
    }

    #[test]
    fn plugin_hooks_are_ok_without_direct_hook_files() {
        let temp = tempfile::tempdir().unwrap();
        let ai_home = temp.path().join(".ai-handoff");
        let codex_plugin = temp.path().join(".agents/plugins/ai-handoff");
        let claude_plugin = temp.path().join(".claude/skills/ai-handoff");
        let hooks = r#"{
  "hooks": {
    "SessionStart": [{"hooks": [{"_aiHandoff": true}]}],
    "UserPromptSubmit": [{"hooks": [{"_aiHandoff": true}]}],
    "PostToolUse": [{"hooks": [{"_aiHandoff": true}]}],
    "Stop": [{"hooks": [{"_aiHandoff": true}]}]
  }
}"#;
        // Codex ships hooks inside the plugin bundle; Claude does not — its hooks
        // live in ~/.claude/settings.json and the plugin root has no hooks.json.
        write(&codex_plugin.join("hooks/hooks.json"), hooks);
        write(
            &temp.path().join(".codex/config.toml"),
            r#"[plugins."ai-handoff@claude-codex-auto-handoff"]
enabled = true

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:SessionStart:0:0"]
trusted_hash = "sha256:trusted-v2"

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:UserPromptSubmit:0:0"]
trusted_hash = "sha256:trusted-v2"

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:PostToolUse:0:0"]
trusted_hash = "sha256:trusted-v2"

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:Stop:0:0"]
trusted_hash = "sha256:trusted-v2"
"#,
        );
        write(&temp.path().join(".claude/settings.json"), hooks);
        state::save(
            &ai_home,
            &state::InstallState {
                codex: state::CodexState {
                    plugin: Some(state::PluginRecord {
                        root: codex_plugin.to_string_lossy().into_owned(),
                        files: vec!["hooks/hooks.json".into()],
                        marketplace_file: Some(
                            temp.path()
                                .join(".agents/plugins/marketplace.json")
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    }),
                    ..Default::default()
                },
                claude: state::ClaudeState {
                    plugin: Some(state::PluginRecord {
                        root: claude_plugin.to_string_lossy().into_owned(),
                        files: vec![".claude-plugin/plugin.json".into()],
                        marketplace_file: None,
                    }),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();

        let snapshot = dashboard_snapshot_for(&ai_home, temp.path());

        assert_eq!(snapshot.codex_hooks.status, CheckStatus::Ok);
        assert_eq!(snapshot.codex_hooks.message, "v2 hooks installed");
        assert_eq!(snapshot.claude_settings.status, CheckStatus::Ok);
        assert_eq!(snapshot.claude_settings.message, "v2 hooks installed");
    }

    #[test]
    fn capsule_list_reports_pending_and_skipped_files() {
        let temp = tempfile::tempdir().unwrap();
        let good = capsule("cap_20260625_010101_abcd", "proj-a", "2026-06-25T01:01:01Z");
        let good_path = temp
            .path()
            .join("store/capsules/proj-a/cap_20260625_010101_abcd.json");
        write(&good_path, &serde_json::to_string_pretty(&good).unwrap());
        let md = capsule("cap_20260625_020202_abcd", "proj-a", "2026-06-25T02:02:02Z");
        let md_path = temp
            .path()
            .join("store/capsules/proj-a/cap_20260625_020202_abcd.md");
        crate::capsule_codec::write_capsule(&md_path, &md, crate::config::CapsuleFormat::Md)
            .unwrap();
        write(
            &temp.path().join("store/capsules/proj-a/bad.json"),
            "{bad json",
        );

        let list = list_capsules_for(temp.path());

        assert_eq!(list.items.len(), 2);
        assert_eq!(list.skipped, 1);
        assert_eq!(list.pending_count, 2);
        assert_eq!(list.items[0].capsule_id, "cap_20260625_020202_abcd");
        assert_eq!(list.items[1].capsule_id, "cap_20260625_010101_abcd");
        assert_eq!(list.items[1].summary_preview, "ship dashboard");
    }

    #[test]
    fn capsule_list_uses_project_label_sidecar() {
        let temp = tempfile::tempdir().unwrap();
        let good = capsule(
            "cap_20260625_010101_abcd",
            "fbaadf85a8ab14c83af2cacc",
            "2026-06-25T01:01:01Z",
        );
        let project_dir = temp.path().join("store/capsules/fbaadf85a8ab14c83af2cacc");
        write(&project_dir.join("project.label"), "ai-handoff\n");
        write(
            &project_dir.join("cap_20260625_010101_abcd.json"),
            &serde_json::to_string_pretty(&good).unwrap(),
        );

        let list = list_capsules_for(temp.path());

        assert_eq!(list.items[0].project_id, "fbaadf85a8ab14c83af2cacc");
        assert_eq!(list.items[0].project_label, "ai-handoff");
    }

    #[test]
    fn capsule_list_labels_current_project_fingerprint_from_cwd() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("ai-handoff");
        std::fs::create_dir_all(&cwd).unwrap();
        let project_id = crate::fingerprint::fingerprint(&cwd);
        let good = capsule(
            "cap_20260625_010101_abcd",
            &project_id,
            "2026-06-25T01:01:01Z",
        );
        write(
            &temp
                .path()
                .join("store/capsules")
                .join(&project_id)
                .join("cap_20260625_010101_abcd.json"),
            &serde_json::to_string_pretty(&good).unwrap(),
        );

        let list = list_capsules_for_with_current(temp.path(), Some(&cwd));

        assert_eq!(list.items[0].project_label, "ai-handoff");
    }
}
