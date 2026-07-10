use crate::{CheckpointAction, CheckpointFormatArg};
use ai_handoff_core::{
    checkpoint_input::parse_checkpoint_input,
    config::{self, CapsuleConfig, CapsuleFormat, Language},
};

use ai_handoff_ipc::protocol::{ClientInfo, Request, Status, VERSION};
use anyhow::{bail, Context};
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use std::io::{Read, Write};

const CAPSULE_FORMAT_FIELD: &str = "_ai_handoff_capsule_format";
const EPISODE_ID_FIELD: &str = "_ai_handoff_episode_id";

struct CheckpointRequestOptions {
    message: Option<String>,
    agent: Option<String>,
    target: Option<String>,
    episode: Option<String>,
    requested_format: Option<CapsuleFormat>,
    autostart_daemon: bool,
}

// This public bridge mirrors the independent clap fields. Internal processing
// immediately groups them into `CheckpointRequestOptions`.
#[allow(clippy::too_many_arguments)]
pub fn run(
    action: Option<CheckpointAction>,
    format: Option<CheckpointFormatArg>,
    json_output: bool,
    message: Option<String>,
    agent: Option<String>,
    target: Option<String>,
    episode: Option<String>,
    file: Option<std::path::PathBuf>,
) -> anyhow::Result<i32> {
    if action == Some(CheckpointAction::Guidance) {
        let agent = normalize_agent(agent.as_deref().unwrap_or("codex"));
        let requested_format = format.map(format_arg);
        let guidance = build_guidance(&agent, requested_format, config::load().capsule);
        let text = if json_output {
            serde_json::to_string(&guidance)
        } else {
            serde_json::to_string_pretty(&guidance)
        }?;
        println!("{text}");
        return Ok(0);
    }
    if json_output {
        bail!("--json is only valid with checkpoint guidance");
    }

    let requested_format = checkpoint_format(action, format)?;

    // --file bypasses stdin, which several shells (notably PowerShell) do not
    // pipe to native executables reliably; fall back to stdin when absent.
    let mut raw_text = String::new();
    if let Some(path) = file {
        raw_text = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read capsule file {}", path.display()))?;
    } else {
        let stdin = std::io::stdin();
        let _ = stdin.lock().read_to_string(&mut raw_text);
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    Ok(run_io_with_format(
        CheckpointRequestOptions {
            message,
            agent,
            target,
            episode,
            requested_format,
            autostart_daemon: true,
        },
        &raw_text,
        &mut out,
    ))
}

pub fn run_io(
    message: Option<String>,
    agent: Option<String>,
    target: Option<String>,
    raw_text: &str,
    out: &mut dyn Write,
) -> i32 {
    run_io_with_format(
        CheckpointRequestOptions {
            message,
            agent,
            target,
            episode: None,
            requested_format: None,
            autostart_daemon: false,
        },
        raw_text,
        out,
    )
}

pub fn run_io_with_autostart(
    message: Option<String>,
    agent: Option<String>,
    target: Option<String>,
    raw_text: &str,
    out: &mut dyn Write,
    autostart_daemon: bool,
) -> i32 {
    run_io_with_format(
        CheckpointRequestOptions {
            message,
            agent,
            target,
            episode: None,
            requested_format: None,
            autostart_daemon,
        },
        raw_text,
        out,
    )
}

fn run_io_with_format(
    options: CheckpointRequestOptions,
    raw_text: &str,
    out: &mut dyn Write,
) -> i32 {
    let input_json = match prepare_input(raw_text, options.requested_format) {
        Ok(value) => value,
        Err(error) => {
            let _ = writeln!(
                out,
                "{}",
                json!({
                    "status": "error",
                    "error": "invalid_checkpoint_input",
                    "message": error.to_string(),
                })
            );
            return 2;
        }
    };
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let message = options
        .message
        .or_else(|| {
            input_json
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "manual checkpoint".to_string());
    // Source agent sets the handoff direction. Precedence: --agent flag, then a
    // stdin `agent` field, then default codex. Normalize aliases to the values
    // the daemon's parse_agent accepts.
    let agent = options
        .agent
        .or_else(|| {
            input_json
                .get("agent")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .map(|value| normalize_agent(&value))
        .unwrap_or_else(|| "codex".to_string());

    let mut raw_hook_input = input_json;
    attach_episode_id(&mut raw_hook_input, options.episode.as_deref());
    if let Some(obj) = raw_hook_input.as_object_mut() {
        obj.insert("cwd".to_string(), json!(cwd.clone()));
        obj.entry("message".to_string())
            .or_insert_with(|| json!(message.clone()));
        // --target beats a stdin `target` field; without either the capsule
        // stays open (no target) — the daemon never guesses a recipient.
        if let Some(target) = &options.target {
            obj.insert("target".to_string(), json!(normalize_agent(target)));
        }
    }

    let req = Request {
        version: VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: "checkpoint".to_string(),
        agent: agent.clone(),
        event: "checkpoint".to_string(),
        received_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        cwd: cwd.clone(),
        session_id: None,
        turn_id: None,
        raw_hook_input,
        client: ClientInfo {
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            platform: std::env::consts::OS.to_string(),
        },
    };

    let resp = crate::daemon_supply::send_with_supply(&req, options.autostart_daemon);
    if resp.status == Status::Ok {
        let text = serde_json::to_string(&resp.hook_stdout).unwrap_or_else(|_| "{}".to_string());
        let _ = writeln!(out, "{text}");
        0
    } else {
        let text = serde_json::to_string(&json!({
            "status": resp.status,
            "warnings": resp.warnings,
            "diagnostics": resp.diagnostics,
        }))
        .unwrap_or_else(|_| r#"{"status":"error"}"#.to_string());
        let _ = writeln!(out, "{text}");
        1
    }
}

fn checkpoint_format(
    action: Option<CheckpointAction>,
    flag: Option<CheckpointFormatArg>,
) -> anyhow::Result<Option<CapsuleFormat>> {
    let action_format = match action {
        Some(CheckpointAction::Md) => Some(CapsuleFormat::Md),
        Some(CheckpointAction::Json) => Some(CapsuleFormat::Json),
        Some(CheckpointAction::Guidance) | None => None,
    };
    let flag_format = flag.map(format_arg);
    if action_format.is_some() && flag_format.is_some() && action_format != flag_format {
        bail!("checkpoint format action conflicts with --format");
    }
    Ok(action_format.or(flag_format))
}

fn format_arg(value: CheckpointFormatArg) -> CapsuleFormat {
    match value {
        CheckpointFormatArg::Md => CapsuleFormat::Md,
        CheckpointFormatArg::Json => CapsuleFormat::Json,
    }
}

fn prepare_input(
    raw_text: &str,
    requested_format: Option<CapsuleFormat>,
) -> Result<Value, ai_handoff_core::checkpoint_input::CheckpointInputError> {
    let mut input = parse_checkpoint_input(raw_text, requested_format)?;
    if let Some(object) = input.as_object_mut() {
        object.remove(CAPSULE_FORMAT_FIELD);
        if let Some(format) = requested_format {
            object.insert(
                CAPSULE_FORMAT_FIELD.to_string(),
                json!(config::capsule_format_str(format)),
            );
        }
    }
    Ok(input)
}

fn build_guidance(
    agent: &str,
    requested_format: Option<CapsuleFormat>,
    capsule: CapsuleConfig,
) -> Value {
    let agent = normalize_agent(agent);
    let format = requested_format.unwrap_or(capsule.format);
    let format_name = config::capsule_format_str(format);
    json!({
        "schema_version": 1,
        "agent": agent,
        "language": config::lang_str(capsule.language),
        "input_format": format_name,
        "storage_format": format_name,
        "limits": {
            "next_prompt_max_items": capsule.next_prompt_limit(),
            "remaining_max_items": capsule.remaining_limit(),
            "done_max_items": capsule.done_limit(),
            "risks_max_items": capsule.risks_limit(),
        },
        "input_template": input_template(format, capsule.language),
        "command": format!(
            "ai-handoff checkpoint {format_name} --agent {agent} --file <path-to.{format_name}>"
        ),
    })
}

fn input_template(format: CapsuleFormat, language: Language) -> &'static str {
    match (format, language) {
        (CapsuleFormat::Json, Language::Ko) => {
            r#"{
  "goal": "<짧은 목표>",
  "done": ["<완료 항목>"],
  "remaining": ["<남은 작업>"],
  "risks": ["<위험 요소>"],
  "next_prompt": "<다음 작업 지시>"
}"#
        }
        (CapsuleFormat::Json, Language::Ja) => {
            r#"{
  "goal": "<短い目標>",
  "done": ["<完了項目>"],
  "remaining": ["<残りの作業>"],
  "risks": ["<リスク>"],
  "next_prompt": "<次の作業指示>"
}"#
        }
        (CapsuleFormat::Json, Language::En) => {
            r#"{
  "goal": "<short goal>",
  "done": ["<completed item>"],
  "remaining": ["<remaining task>"],
  "risks": ["<risk>"],
  "next_prompt": "<next work instruction>"
}"#
        }
        (CapsuleFormat::Md, Language::Ko) => {
            "# AI Handoff Checkpoint\n\n## 목표\n\n<짧은 목표>\n\n## 완료\n\n- <완료 항목>\n\n## 남은 작업\n\n- <남은 작업>\n\n## 위험 요소\n\n- <위험 요소>\n\n## 다음 프롬프트\n\n- <다음 작업 지시>\n"
        }
        (CapsuleFormat::Md, Language::Ja) => {
            "# AI Handoff Checkpoint\n\n## 目標\n\n<短い目標>\n\n## 完了\n\n- <完了項目>\n\n## 残りの作業\n\n- <残りの作業>\n\n## リスク\n\n- <リスク>\n\n## 次のプロンプト\n\n- <次の作業指示>\n"
        }
        (CapsuleFormat::Md, Language::En) => {
            "# AI Handoff Checkpoint\n\n## Goal\n\n<short goal>\n\n## Done\n\n- <completed item>\n\n## Remaining\n\n- <remaining task>\n\n## Risks\n\n- <risk>\n\n## Next Prompt\n\n- <next work instruction>\n"
        }
    }
}

fn normalize_agent(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claude_code" | "claudecode" => "claude-code".to_string(),
        "codex" => "codex".to_string(),
        other => other.to_string(),
    }
}

fn attach_episode_id(input: &mut Value, episode_id: Option<&str>) {
    let Some(episode_id) = episode_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return;
    };
    if let Some(object) = input.as_object_mut() {
        object.insert(EPISODE_ID_FIELD.to_string(), json!(episode_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_core::config::{CapsuleConfig, CapsuleFormat, Language};

    fn capsule_config(format: CapsuleFormat) -> CapsuleConfig {
        CapsuleConfig {
            format,
            language: Language::Ko,
            next_prompt_max_items: 10,
            remaining_max_items: 10,
            done_max_items: 10,
            risks_max_items: 10,
        }
    }

    #[test]
    fn guidance_reports_resolved_settings_for_codex_markdown() {
        let guidance = build_guidance("codex", None, capsule_config(CapsuleFormat::Md));

        assert_eq!(guidance["agent"], "codex");
        assert_eq!(guidance["language"], "ko");
        assert_eq!(guidance["input_format"], "md");
        assert_eq!(guidance["storage_format"], "md");
        assert_eq!(guidance["limits"]["next_prompt_max_items"], 10);
        assert_eq!(guidance["limits"]["remaining_max_items"], 10);
        assert_eq!(guidance["limits"]["done_max_items"], 10);
        assert_eq!(guidance["limits"]["risks_max_items"], 10);
        assert!(guidance["input_template"]
            .as_str()
            .unwrap()
            .contains("## 목표"));
        assert_eq!(
            guidance["command"],
            "ai-handoff checkpoint md --agent codex --file <path-to.md>"
        );
    }

    #[test]
    fn guidance_normalizes_claude_and_honors_explicit_json() {
        let guidance = build_guidance(
            "claude",
            Some(CapsuleFormat::Json),
            capsule_config(CapsuleFormat::Md),
        );

        assert_eq!(guidance["agent"], "claude-code");
        assert_eq!(guidance["input_format"], "json");
        assert_eq!(guidance["storage_format"], "json");
        assert!(guidance["input_template"]
            .as_str()
            .unwrap()
            .contains("\"goal\""));
        assert_eq!(
            guidance["command"],
            "ai-handoff checkpoint json --agent claude-code --file <path-to.json>"
        );
    }

    #[test]
    fn explicit_format_is_added_only_to_internal_checkpoint_payload() {
        let explicit = prepare_input("## Goal\nkeep working", Some(CapsuleFormat::Md)).unwrap();
        assert_eq!(explicit["_ai_handoff_capsule_format"], "md");
        assert_eq!(explicit["goal"], "keep working");
        let forged = prepare_input(
            r#"{"goal":"legacy","_ai_handoff_capsule_format":"md"}"#,
            None,
        )
        .unwrap();
        assert!(forged.get("_ai_handoff_capsule_format").is_none());

        let automatic = prepare_input(r#"{"goal":"legacy"}"#, None).unwrap();
        assert!(automatic.get("_ai_handoff_capsule_format").is_none());
        assert_eq!(automatic["goal"], "legacy");
    }

    #[test]
    fn explicit_episode_id_is_attached_to_checkpoint_payload() {
        let mut input = serde_json::json!({ "goal": "keep working" });

        attach_episode_id(&mut input, Some("episode-123"));

        assert_eq!(input["_ai_handoff_episode_id"], "episode-123");
    }
}
