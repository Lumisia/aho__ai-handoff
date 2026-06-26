//! Detect leftover ai-handoff integrations that would double-fire alongside
//! the v2 plugin bundle.
//!
//! This module is **advisory only** — all detection is best-effort.
//! Malformed or absent inputs are silently skipped; no panics, no hard errors.

use serde_json::Value;
use toml_edit::DocumentMut;

use super::codex_config::PLUGIN_ENABLE_KEY;

/// A single finding produced by [`detect`].
#[derive(Debug, PartialEq)]
pub struct DuplicateFinding {
    /// Which agent the finding belongs to: `"codex"` or `"claude"`.
    pub agent: &'static str,
    /// Human-readable guidance describing the conflict and how to resolve it.
    pub detail: String,
}

/// Scan Codex and Claude config text for leftover integrations that would
/// double-fire with the v2 plugin bundle.
///
/// # Codex
/// Parses `codex_config_text` as TOML using `toml_edit`.
///
/// - Legacy v1 plugin trust keys are detected by their old hook path
///   (`hooks-codex.json` / monitors), not by `ai-handoff@` alone. The v2
///   plugin's own trusted hook keys also use `ai-handoff@`, so treating all
///   such keys as v1 is a false positive.
/// - If the v2 plugin is active (already enabled in config, or this scan is
///   running for a plugin-mode install), user-level `~/.codex/hooks.json`
///   entries carrying `_aiHandoff:true` are flagged as direct-hook residue.
///
/// # Claude
/// Parses `claude_settings_text` as JSON. If `enabledPlugins` contains any
/// `ai-handoff@* = true`, a finding is returned for the legacy plugin. During a
/// plugin-mode install, direct managed `hooks` entries are also flagged.
///
/// # Error handling
/// `None` inputs and parse failures are silently ignored — this function
/// never panics and always returns (possibly empty) `Vec`.
pub fn detect(
    codex_config_text: Option<&str>,
    codex_hooks_text: Option<&str>,
    claude_settings_text: Option<&str>,
    installing_plugin: bool,
) -> Vec<DuplicateFinding> {
    let mut findings = Vec::new();
    let mut codex_v2_plugin_active = installing_plugin;

    // --- Codex ---
    if let Some(text) = codex_config_text {
        if let Ok(doc) = text.parse::<DocumentMut>() {
            codex_v2_plugin_active |= codex_plugin_enabled_doc(&doc);
            if let Some(hook_keys) = hooks_state_keys(&doc) {
                let v1_keys: Vec<String> = hook_keys
                    .into_iter()
                    .filter(|k| is_legacy_codex_plugin_hook_key(k))
                    .collect();
                if !v1_keys.is_empty() {
                    findings.push(DuplicateFinding {
                        agent: "codex",
                        detail: format!(
                            "Leftover v1 ai-handoff plugin hook(s) detected in Codex hooks.state: \
                             {}. These will double-fire alongside v2 user-level hooks. \
                             Open Codex `/hooks`, locate the ai-handoff entries, and choose \
                             \"Reject\" or \"Disable\" to remove them.",
                            v1_keys.join(", ")
                        ),
                    });
                }
            }
        }
    }

    if codex_v2_plugin_active {
        if let Some(text) = codex_hooks_text {
            if json_hooks_have_ai_handoff(text) {
                findings.push(DuplicateFinding {
                    agent: "codex",
                    detail: "Direct ai-handoff entries detected in Codex hooks.json while the \
                             v2 plugin is enabled. These user-level hooks will double-fire with \
                             the plugin bundle. Remove the direct hooks (for example by running \
                             ai-handoff uninstall from the old direct-hook install) or use \
                             ai-handoff install --no-plugin."
                        .to_string(),
                });
            }
        }
    }

    // --- Claude ---
    if let Some(text) = claude_settings_text {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
            if let Some(plugins) = val.get("enabledPlugins").and_then(|v| v.as_object()) {
                let v1_keys: Vec<&str> = plugins
                    .iter()
                    .filter(|(k, v)| k.starts_with("ai-handoff@") && v.as_bool() == Some(true))
                    .map(|(k, _)| k.as_str())
                    .collect();
                if !v1_keys.is_empty() {
                    findings.push(DuplicateFinding {
                        agent: "claude",
                        detail: format!(
                            "Leftover v1 ai-handoff plugin(s) enabled in Claude settings: \
                             {}. These will double-fire alongside v2 user-level hooks. \
                             Set the plugin to false in your Claude settings \
                             (`enabledPlugins[\"{}\"] = false`) or uninstall the v1 plugin.",
                            v1_keys.join(", "),
                            v1_keys.first().copied().unwrap_or("ai-handoff@...")
                        ),
                    });
                }
            }
            if installing_plugin && value_has_ai_handoff_hooks(&val) {
                findings.push(DuplicateFinding {
                    agent: "claude",
                    detail: "Direct ai-handoff entries detected in Claude settings hooks while \
                             the v2 plugin is being installed. These hooks will double-fire with \
                             the plugin bundle. Remove the direct hooks with the old install \
                             state or keep direct-hook mode with ai-handoff install --no-plugin."
                        .to_string(),
                });
            }
        }
    }

    findings
}

/// Extract all keys from the `[hooks.state]` table in a parsed Codex config,
/// returning `None` if the table path does not exist.
fn hooks_state_keys(doc: &DocumentMut) -> Option<Vec<String>> {
    let state = doc.get("hooks")?.as_table()?.get("state")?.as_table()?;
    Some(state.iter().map(|(k, _)| k.to_string()).collect())
}

fn is_legacy_codex_plugin_hook_key(key: &str) -> bool {
    key.starts_with("ai-handoff@")
        && (key.contains("hooks-codex.json") || key.contains("monitors/monitors.json"))
}

fn is_v2_codex_plugin_hook_key(key: &str) -> bool {
    key.starts_with("ai-handoff@") && key.contains("hooks/hooks.json")
}

pub fn codex_v2_plugin_enabled(config_text: &str) -> bool {
    config_text
        .parse::<DocumentMut>()
        .map(|doc| codex_plugin_enabled_doc(&doc))
        .unwrap_or(false)
}

pub fn codex_v2_plugin_trusted(config_text: &str) -> bool {
    config_text
        .parse::<DocumentMut>()
        .ok()
        .and_then(|doc| hooks_state_keys(&doc))
        .map(|keys| {
            super::codex_hooks::EVENTS.iter().all(|event| {
                keys.iter()
                    .any(|key| is_v2_codex_plugin_hook_for_event(key, event))
            })
        })
        .unwrap_or(false)
}

fn codex_plugin_enabled_doc(doc: &DocumentMut) -> bool {
    doc.get("plugins")
        .and_then(|item| item.as_table())
        .and_then(|plugins| plugins.get(PLUGIN_ENABLE_KEY))
        .and_then(|item| item.as_table())
        .and_then(|table| table.get("enabled"))
        .and_then(|item| item.as_bool())
        == Some(true)
}

fn json_hooks_have_ai_handoff(text: &str) -> bool {
    serde_json::from_str::<Value>(text)
        .map(|value| value_has_ai_handoff_hooks(&value))
        .unwrap_or(false)
}

fn value_has_ai_handoff_hooks(value: &Value) -> bool {
    value
        .get("hooks")
        .and_then(Value::as_object)
        .map(|events| {
            events.values().any(|event| {
                event.as_array().is_some_and(|outer_entries| {
                    outer_entries.iter().any(|outer| {
                        outer
                            .get("hooks")
                            .and_then(Value::as_array)
                            .is_some_and(|inner_hooks| {
                                inner_hooks.iter().any(|hook| {
                                    hook.get("_aiHandoff").and_then(Value::as_bool) == Some(true)
                                })
                            })
                    })
                })
            })
        })
        .unwrap_or(false)
}

fn is_v2_codex_plugin_hook_for_event(key: &str, event: &str) -> bool {
    is_v2_codex_plugin_hook_key(key)
        && normalized_hook_key(key).contains(&normalized_hook_key(event))
}

fn normalized_hook_key(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codex_fixture() -> String {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codex-config-complex.toml"
        ))
        .unwrap()
    }

    // (1) Fixture codex config → exactly one codex finding
    #[test]
    fn detects_v1_codex_hook_in_fixture() {
        let findings = detect(Some(&codex_fixture()), None, None, false);
        assert_eq!(findings.len(), 1, "expected exactly one finding");
        assert_eq!(findings[0].agent, "codex");
        assert!(
            findings[0].detail.contains("ai-handoff@"),
            "detail should mention the v1 plugin key"
        );
        assert!(
            findings[0].detail.to_lowercase().contains("reject")
                || findings[0].detail.to_lowercase().contains("disable"),
            "detail should include remediation guidance"
        );
    }

    // (2) Claude settings with ai-handoff@ plugin enabled → one claude finding
    #[test]
    fn codex_v2_trusted_plugin_hook_is_not_flagged_as_v1() {
        let cfg = r#"
[plugins."ai-handoff@claude-codex-auto-handoff"]
enabled = true

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:SessionStart:0:0"]
trusted_hash = "sha256:trusted-v2"
"#;
        let findings = detect(Some(cfg), None, None, false);
        assert!(
            findings.is_empty(),
            "v2 plugin trust state must not be reported as legacy duplicate: {findings:?}"
        );
    }

    #[test]
    fn codex_v2_plugin_trusted_requires_all_events() {
        let partial = r#"
[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:SessionStart:0:0"]
trusted_hash = "sha256:trusted-v2"
"#;
        assert!(!codex_v2_plugin_trusted(partial));

        let full = r#"
[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:SessionStart:0:0"]
trusted_hash = "sha256:trusted-v2"

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:UserPromptSubmit:0:0"]
trusted_hash = "sha256:trusted-v2"

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:PostToolUse:0:0"]
trusted_hash = "sha256:trusted-v2"

[hooks.state."ai-handoff@claude-codex-auto-handoff:hooks/hooks.json:Stop:0:0"]
trusted_hash = "sha256:trusted-v2"
"#;
        assert!(codex_v2_plugin_trusted(full));
    }

    #[test]
    fn detects_v1_claude_plugin_enabled() {
        let settings = r#"{"enabledPlugins":{"ai-handoff@cm":true}}"#;
        let findings = detect(None, None, Some(settings), false);
        assert_eq!(findings.len(), 1, "expected exactly one finding");
        assert_eq!(findings[0].agent, "claude");
        assert!(
            findings[0].detail.contains("ai-handoff@cm"),
            "detail should name the offending plugin"
        );
        assert!(
            findings[0].detail.to_lowercase().contains("false")
                || findings[0].detail.to_lowercase().contains("uninstall"),
            "detail should include remediation guidance"
        );
    }

    // (3) Clean inputs → empty findings
    #[test]
    fn detects_codex_direct_hooks_when_v2_plugin_is_enabled() {
        let config = r#"
[plugins."ai-handoff@claude-codex-auto-handoff"]
enabled = true
"#;
        let hooks = r#"{
  "hooks": {
    "Stop": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "\"C:/p/ai-handoff.exe\" hook stop --agent codex",
            "_aiHandoff": true
          }
        ]
      }
    ]
  }
}"#;
        let findings = detect(Some(config), Some(hooks), None, false);
        assert_eq!(findings.len(), 1, "expected direct-hook duplicate");
        assert_eq!(findings[0].agent, "codex");
        assert!(findings[0].detail.contains("hooks.json"));
        assert!(findings[0].detail.contains("plugin"));
    }

    #[test]
    fn codex_direct_hooks_are_not_duplicate_without_plugin_mode() {
        let hooks = r#"{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"C:/p/ai-handoff.exe\" hook stop --agent codex",
            "_aiHandoff": true
          }
        ]
      }
    ]
  }
}"#;
        let findings = detect(None, Some(hooks), None, false);
        assert!(
            findings.is_empty(),
            "direct hooks alone are valid in --no-plugin mode"
        );
    }

    #[test]
    fn clean_inputs_produce_no_findings() {
        let clean_codex = r#"
model = "gpt-5.5"
approval_policy = "on-request"

[hooks.state."some-other-plugin@vendor:hooks/hooks.json:session_start:0:0"]
trusted_hash = "sha256:aabbcc"
"#;
        let clean_claude = r#"{"enabledPlugins":{"some-other-plugin@vendor":true}}"#;
        let findings = detect(Some(clean_codex), None, Some(clean_claude), false);
        assert!(
            findings.is_empty(),
            "expected no findings for clean inputs, got: {findings:?}"
        );
    }

    // (3b) Both None → empty
    #[test]
    fn none_inputs_produce_no_findings() {
        let findings = detect(None, None, None, false);
        assert!(findings.is_empty());
    }

    // (4) Malformed inputs → empty, no panic
    #[test]
    fn malformed_codex_is_skipped_gracefully() {
        let findings = detect(Some("not = = valid toml !!!"), None, None, false);
        assert!(
            findings.is_empty(),
            "expected no findings for malformed TOML"
        );
    }

    #[test]
    fn malformed_claude_is_skipped_gracefully() {
        let findings = detect(None, None, Some("{not json at all"), false);
        assert!(
            findings.is_empty(),
            "expected no findings for malformed JSON"
        );
    }

    // (4b) Disabled claude plugin → no finding (value is false)
    #[test]
    fn disabled_claude_plugin_is_not_flagged() {
        let settings = r#"{"enabledPlugins":{"ai-handoff@cm":false}}"#;
        let findings = detect(None, None, Some(settings), false);
        assert!(
            findings.is_empty(),
            "a disabled plugin should not be flagged"
        );
    }

    // Both agents fire simultaneously
    #[test]
    fn detects_both_agents_when_both_have_v1_hooks() {
        let settings = r#"{"enabledPlugins":{"ai-handoff@cm":true}}"#;
        let findings = detect(Some(&codex_fixture()), None, Some(settings), false);
        assert_eq!(findings.len(), 2);
        let agents: Vec<&str> = findings.iter().map(|f| f.agent).collect();
        assert!(agents.contains(&"codex"));
        assert!(agents.contains(&"claude"));
    }
}
