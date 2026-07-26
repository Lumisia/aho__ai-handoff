use super::managed_hook::{without_managed_inner_hooks, AGENT_CLAUDE, MANAGED_EVENTS};

/// Apply our managed hooks into `existing` JSON (or a blank object when `None`).
///
/// For each of the 4 events the function:
/// 1. Gets or creates the event array under `hooks.<Event>`.
/// 2. Removes only our managed inner hooks (see [`super::managed_hook`]),
///    preserving foreign inner hooks and their outer metadata.
/// 3. Appends exactly one new outer entry containing our managed inner hook in exec form.
///
/// Ownership is NOT decided by the `_aiHandoff` marker alone: Claude Code
/// rewrites settings.json and drops that unknown key, and marker-only matching
/// then re-classified our own entry as foreign and appended a duplicate on
/// every install. See [`super::managed_hook`] for the full matching rule.
///
/// PostToolUse additionally carries `"matcher": "Write|Edit|Bash|AskUserQuestion"`
/// so the selected checkpoint decision returns through the same lifecycle hook.
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

    for (event, event_arg) in MANAGED_EVENTS {
        let hooks_obj = root["hooks"].as_object_mut().unwrap();

        // Remove only our inner hooks, preserving foreign hooks and outer metadata.
        let foreign: Vec<Value> = hooks_obj
            .get(event)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|outer| without_managed_inner_hooks(outer, AGENT_CLAUDE, event_arg))
                    .collect()
            })
            .unwrap_or_default();

        // Build our managed inner hook (exec form).
        let inner_hook = json!({
            "type": "command",
            "command": exe,
            "args": ["hook", event_arg, "--agent", "claude-code"],
            "_aiHandoff": true,
            "timeout": 10
        });

        // Build the outer entry (PostToolUse gets a matcher).
        let our_entry = if event == "PostToolUse" {
            json!({
                "matcher": "Write|Edit|Bash|AskUserQuestion",
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

/// Remove every managed inner hook (see [`super::managed_hook`] — marker or
/// invocation, so entries whose marker Claude Code stripped are still cleaned
/// up). Preserve foreign inner hooks sharing the same outer entry. Prune event
/// arrays that become empty after removal.
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

        for (event, event_arg) in MANAGED_EVENTS {
            if let Some(arr) = hooks_obj.get_mut(event).and_then(Value::as_array_mut) {
                *arr = arr
                    .iter()
                    .filter_map(|outer| without_managed_inner_hooks(outer, AGENT_CLAUDE, event_arg))
                    .collect();
                if arr.is_empty() {
                    empty_events.push(event.to_string());
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
    fn apply_and_remove_preserve_foreign_inner_hook_in_a_mixed_outer() {
        let existing = r#"{
          "hooks": {
            "Stop": [{
              "matcher": "user-matcher",
              "custom": true,
              "hooks": [
                {
                  "type": "command",
                  "command": "C:\\p\\ai-handoff.exe",
                  "args": ["hook", "stop", "--agent", "claude-code"]
                },
                {"type": "command", "command": "user-tool --save"}
              ]
            }]
          }
        }"#;

        let (applied, _) = apply(Some(existing), "C:\\p\\ai-handoff.exe").unwrap();
        let applied: Value = serde_json::from_str(&applied).unwrap();
        let stop = applied["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "foreign outer plus new managed outer");
        assert_eq!(stop[0]["matcher"], "user-matcher");
        assert_eq!(stop[0]["custom"], true);
        assert_eq!(stop[0]["hooks"].as_array().unwrap().len(), 1);
        assert_eq!(stop[0]["hooks"][0]["command"], "user-tool --save");

        let removed = remove(existing).unwrap();
        let removed: Value = serde_json::from_str(&removed).unwrap();
        assert_eq!(removed["hooks"]["Stop"][0]["matcher"], "user-matcher");
        assert_eq!(removed["hooks"]["Stop"][0]["custom"], true);
        assert_eq!(
            removed["hooks"]["Stop"][0]["hooks"][0]["command"],
            "user-tool --save"
        );
    }

    #[test]
    fn post_tool_use_has_matcher() {
        let (json_str, _) = apply(None, "C:\\p\\ai-handoff.exe").unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(
            v["hooks"]["PostToolUse"][0]["matcher"],
            "Write|Edit|Bash|AskUserQuestion"
        );
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

    /// Strip the `_aiHandoff` marker from every inner hook, reproducing what
    /// Claude Code does when it re-serializes settings.json.
    fn strip_markers(text: &str) -> String {
        let mut root: Value = serde_json::from_str(text).unwrap();
        let hooks = root["hooks"].as_object_mut().unwrap();
        for arr in hooks.values_mut() {
            for outer in arr.as_array_mut().unwrap() {
                for inner in outer["hooks"].as_array_mut().unwrap() {
                    inner.as_object_mut().unwrap().remove("_aiHandoff");
                }
            }
        }
        serde_json::to_string_pretty(&root).unwrap()
    }

    #[test]
    fn apply_is_idempotent_after_the_marker_is_stripped() {
        // Regression: Claude Code drops the unknown `_aiHandoff` key when it
        // rewrites settings.json. Marker-only matching then read our own entry
        // as foreign and appended a second copy on every install, growing the
        // array 1 -> 2 -> 3 across releases.
        let exe = "C:\\p\\ai-handoff.exe";
        let (first, _) = apply(None, exe).unwrap();
        let stripped = strip_markers(&first);

        let (second, _) = apply(Some(&stripped), exe).unwrap();
        let v: Value = serde_json::from_str(&second).unwrap();
        for (event, _) in MANAGED_EVENTS {
            assert_eq!(
                v["hooks"][event].as_array().unwrap().len(),
                1,
                "{event} must keep exactly one managed entry"
            );
        }
    }

    #[test]
    fn apply_collapses_pre_existing_unmarked_duplicates() {
        // Settings that already accumulated three unmarked copies per event
        // must come back down to one.
        let exe = "C:\\p\\ai-handoff.exe";
        let (base, _) = apply(None, exe).unwrap();
        let mut root: Value = serde_json::from_str(&strip_markers(&base)).unwrap();
        for arr in root["hooks"].as_object_mut().unwrap().values_mut() {
            let entry = arr[0].clone();
            arr.as_array_mut().unwrap().push(entry.clone());
            arr.as_array_mut().unwrap().push(entry);
        }
        let triple = serde_json::to_string_pretty(&root).unwrap();

        let (repaired, _) = apply(Some(&triple), exe).unwrap();
        let v: Value = serde_json::from_str(&repaired).unwrap();
        for (event, _) in MANAGED_EVENTS {
            assert_eq!(v["hooks"][event].as_array().unwrap().len(), 1, "{event}");
        }
    }

    #[test]
    fn remove_strips_entries_whose_marker_was_stripped() {
        let (applied, _) = apply(None, "C:\\p\\ai-handoff.exe").unwrap();
        let cleaned = remove(&strip_markers(&applied)).unwrap();
        let c: Value = serde_json::from_str(&cleaned).unwrap();
        assert!(c.get("hooks").is_none(), "unmarked leftovers: {cleaned}");
    }

    #[test]
    fn apply_preserves_a_user_hook_that_calls_our_binary_for_something_else() {
        let user = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"\"C:\\p\\ai-handoff.exe\" statusline"}]}]}}"#;
        let (merged, _) = apply(Some(user), "C:\\p\\ai-handoff.exe").unwrap();
        let v: Value = serde_json::from_str(&merged).unwrap();
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "user entry must survive alongside ours");
        assert_eq!(
            stop[0]["hooks"][0]["command"],
            "\"C:\\p\\ai-handoff.exe\" statusline"
        );
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
