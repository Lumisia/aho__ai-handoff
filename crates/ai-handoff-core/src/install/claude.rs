/// Lifecycle event names as they appear in Claude settings.json `hooks` object keys.
pub const EVENTS: [&str; 4] = ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop"];

/// CLI kebab arg strings corresponding to each entry in EVENTS (same order).
const EVENT_ARGS: [&str; 4] = ["session-start", "user-prompt", "post-tool-use", "stop"];

/// Apply our managed hooks into `existing` JSON (or a blank object when `None`).
///
/// For each of the 4 events the function:
/// 1. Gets or creates the event array under `hooks.<Event>`.
/// 2. Retains all outer entries whose inner `hooks[]` array contains NO entry
///    with `_aiHandoff == true` (those are "foreign" entries we preserve).
/// 3. Appends exactly one new outer entry containing our managed inner hook in exec form.
///
/// PostToolUse additionally carries `"matcher": "Write|Edit|Bash"` on the outer entry.
///
/// Returns `Ok((pretty_json, managed_event_names))`, or a parse error when
/// `existing` contains invalid JSON (caller should abort; the backup is the
/// recovery path — never silently clobber).
pub fn apply(existing: Option<&str>, exe: &str) -> serde_json::Result<(String, Vec<String>)> {
    use serde_json::{json, Value};

    let mut root: Value = match existing {
        Some(s) => serde_json::from_str::<Value>(s)?,
        None => json!({}),
    };

    // Ensure `hooks` key is an object.
    if !root["hooks"].is_object() {
        root["hooks"] = json!({});
    }

    let mut managed_events: Vec<String> = Vec::new();

    for (event, event_arg) in EVENTS.iter().zip(EVENT_ARGS.iter()) {
        let hooks_obj = root["hooks"].as_object_mut().unwrap();

        // Collect existing outer entries that are NOT ours (no _aiHandoff inner hook).
        let foreign: Vec<Value> = hooks_obj
            .get(*event)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|outer| {
                        // An outer entry is "ours" if ANY of its inner hooks has _aiHandoff:true.
                        !outer["hooks"]
                            .as_array()
                            .map(|inner_hooks| {
                                inner_hooks
                                    .iter()
                                    .any(|h| h["_aiHandoff"].as_bool() == Some(true))
                            })
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        // Build our managed inner hook (exec form).
        let inner_hook = json!({
            "type": "command",
            "command": exe,
            "args": ["hook", *event_arg, "--agent", "claude-code"],
            "_aiHandoff": true,
            "timeout": 10
        });

        // Build the outer entry (PostToolUse gets a matcher).
        let our_entry = if *event == "PostToolUse" {
            json!({
                "matcher": "Write|Edit|Bash",
                "hooks": [inner_hook]
            })
        } else {
            json!({
                "hooks": [inner_hook]
            })
        };

        let mut new_array = foreign;
        new_array.push(our_entry);

        hooks_obj.insert(event.to_string(), Value::Array(new_array));
        managed_events.push(event.to_string());
    }

    let pretty = serde_json::to_string_pretty(&root).expect("serialization cannot fail");
    Ok((pretty, managed_events))
}

/// Remove every outer hook entry whose inner `hooks[]` array contains an entry
/// with `_aiHandoff == true`.  Prune event arrays that become empty after removal.
/// Prune an empty `hooks` object entirely.
/// Foreign entries are preserved unchanged.
///
/// Returns `Ok(pretty_json)`, or a parse error when `existing` contains invalid
/// JSON (caller should abort rather than overwriting).
pub fn remove(existing: &str) -> serde_json::Result<String> {
    use serde_json::Value;

    let mut root: Value = serde_json::from_str(existing)?;

    if let Some(hooks_obj) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        let mut empty_events: Vec<String> = Vec::new();

        for (event, arr_val) in hooks_obj.iter_mut() {
            if let Some(arr) = arr_val.as_array_mut() {
                arr.retain(|outer| {
                    // Keep outer entries that contain NO _aiHandoff inner hook.
                    !outer["hooks"]
                        .as_array()
                        .map(|inner_hooks| {
                            inner_hooks
                                .iter()
                                .any(|h| h["_aiHandoff"].as_bool() == Some(true))
                        })
                        .unwrap_or(false)
                });
                if arr.is_empty() {
                    empty_events.push(event.clone());
                }
            }
        }

        for ev in &empty_events {
            hooks_obj.remove(ev);
        }
    }

    // Prune an empty `hooks` object.
    if root
        .get("hooks")
        .and_then(Value::as_object)
        .map(|o| o.is_empty())
        .unwrap_or(false)
    {
        root.as_object_mut().map(|o| o.remove("hooks"));
    }

    Ok(serde_json::to_string_pretty(&root).expect("serialization cannot fail"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn merge_preserves_other_keys_and_is_idempotent() {
        let base = r#"{"model":"opus","enabledPlugins":{"x":true}}"#;
        let (a, events) = apply(Some(base), "C:\\p\\ai-handoff.exe").unwrap();
        assert_eq!(events.len(), 4);
        let v: Value = serde_json::from_str(&a).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(v["enabledPlugins"]["x"], true);
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["args"][1], "stop");
        let (b, _) = apply(Some(&a), "C:\\p\\ai-handoff.exe").unwrap();
        let v2: Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v2["hooks"]["Stop"].as_array().unwrap().len(), 1);
        let cleaned = remove(&b).unwrap();
        let c: Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(c["model"], "opus");
        assert!(c
            .get("hooks")
            .map(|h| h.as_object().unwrap().is_empty())
            .unwrap_or(true));
    }

    #[test]
    fn apply_inserts_four_managed_hooks_idempotently() {
        let exe = "C:\\p\\ai-handoff.exe";
        let (first, events) = apply(None, exe).unwrap();
        assert_eq!(events.len(), 4);
        let v: Value = serde_json::from_str(&first).unwrap();
        assert!(v["hooks"]["Stop"][0]["hooks"][0]["_aiHandoff"]
            .as_bool()
            .unwrap());
        // idempotent: re-apply over our own output keeps exactly one managed entry per event
        let (second, _) = apply(Some(&first), exe).unwrap();
        let v2: Value = serde_json::from_str(&second).unwrap();
        assert_eq!(v2["hooks"]["Stop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn apply_preserves_foreign_hooks_and_remove_strips_only_ours() {
        let foreign = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"other"}]}]}}"#;
        let (merged, _) = apply(Some(foreign), "C:\\p\\ai-handoff.exe").unwrap();
        let v: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2); // foreign + ours
        let cleaned = remove(&merged).unwrap();
        let c: Value = serde_json::from_str(&cleaned).unwrap();
        let stop = c["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0]["hooks"][0]["command"], "other");
    }

    #[test]
    fn post_tool_use_has_matcher() {
        let (json_str, _) = apply(None, "C:\\p\\ai-handoff.exe").unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["hooks"]["PostToolUse"][0]["matcher"], "Write|Edit|Bash");
        // Other events should NOT have matcher
        assert!(v["hooks"]["Stop"][0].get("matcher").is_none());
        assert!(v["hooks"]["SessionStart"][0].get("matcher").is_none());
    }

    #[test]
    fn apply_and_remove_return_err_on_invalid_json() {
        let exe = "C:\\p\\ai-handoff.exe";
        assert!(apply(Some("not valid json"), exe).is_err());
        assert!(remove("not valid json").is_err());
    }

    #[test]
    fn remove_prunes_empty_hooks_object() {
        let (applied, _) = apply(None, "C:\\p\\ai-handoff.exe").unwrap();
        let cleaned = remove(&applied).unwrap();
        let c: Value = serde_json::from_str(&cleaned).unwrap();
        // hooks key should be absent or empty after removing all managed entries
        assert!(c.get("hooks").is_none());
    }

    #[test]
    fn remove_does_not_insert_null_hooks_when_hooks_key_is_absent() {
        let cleaned = remove(r#"{"model":"opus"}"#).unwrap();
        let c: Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(c["model"], "opus");
        assert!(c.get("hooks").is_none());
    }
}
