use super::managed_hook::{without_managed_inner_hooks, AGENT_CODEX, MANAGED_EVENTS};

/// Build the shell command string for a managed hook.
///
/// Format: `"<exe>" hook <event_arg> --agent codex`
pub fn managed_command(exe: &str, event_arg: &str) -> String {
    format!("\"{}\" hook {} --agent codex", exe, event_arg)
}

/// Apply our managed hooks into `existing` JSON (or a blank object when `None`).
///
/// For each of the 4 events the function:
/// 1. Gets or creates the event array under `hooks.<Event>`.
/// 2. Removes only our managed inner hooks (see [`super::managed_hook`]),
///    preserving foreign inner hooks and their outer metadata.
/// 3. Appends exactly one new outer entry containing our managed inner hook.
///
/// Ownership does not rely on the `_aiHandoff` marker alone; matching the
/// invocation too keeps re-apply idempotent if a host ever re-serializes
/// hooks.json and drops the unknown key, as Claude Code does to settings.json.
///
/// Returns `Ok((pretty_json, managed_event_names))`, or a parse error when
/// `existing` contains invalid JSON (caller should abort; the backup is the
/// recovery path — never silently clobber).
pub fn apply(existing: Option<&str>, exe: &str) -> serde_json::Result<(String, Vec<String>)> {
    use serde_json::{json, Value};

    let mut root: Value = match existing {
        Some(s) => serde_json::from_str::<Value>(s)?,
        None => json!({"hooks": {}}),
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
                    .filter_map(|outer| without_managed_inner_hooks(outer, AGENT_CODEX, event_arg))
                    .collect()
            })
            .unwrap_or_default();

        // Build our managed outer entry.
        let cmd = managed_command(exe, event_arg);
        let our_entry = json!({
            "matcher": "*",
            "hooks": [
                {
                    "type": "command",
                    "command": cmd,
                    "_aiHandoff": true,
                    "timeout": 10
                }
            ]
        });

        let mut new_array = foreign;
        new_array.push(our_entry);

        hooks_obj.insert(event.to_string(), Value::Array(new_array));
        managed_events.push(event.to_string());
    }

    let pretty = serde_json::to_string_pretty(&root).expect("serialization cannot fail");
    Ok((pretty, managed_events))
}

/// Remove every managed inner hook (see [`super::managed_hook`] — marker or
/// invocation). Preserve foreign inner hooks sharing the same outer entry.
/// Prune event arrays that become empty after removal.
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
                    .filter_map(|outer| without_managed_inner_hooks(outer, AGENT_CODEX, event_arg))
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

    Ok(serde_json::to_string_pretty(&root).expect("serialization cannot fail"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

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
        let foreign = r#"{"hooks":{"Stop":[{"matcher":"*","hooks":[{"type":"command","command":"other"}]}]}}"#;
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
                  "command": "\"C:\\p\\ai-handoff.exe\" hook stop --agent codex"
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
    fn apply_and_remove_return_err_on_invalid_json() {
        let exe = "C:\\p\\ai-handoff.exe";
        assert!(apply(Some("not valid json"), exe).is_err());
        assert!(remove("not valid json").is_err());
    }

    #[test]
    fn apply_is_idempotent_after_the_marker_is_stripped() {
        // Parity with install::claude — a host that re-serializes hooks.json and
        // drops the unknown `_aiHandoff` key must not cause duplicate entries.
        let exe = "C:\\p\\ai-handoff.exe";
        let (first, _) = apply(None, exe).unwrap();
        let mut root: Value = serde_json::from_str(&first).unwrap();
        for arr in root["hooks"].as_object_mut().unwrap().values_mut() {
            for outer in arr.as_array_mut().unwrap() {
                for inner in outer["hooks"].as_array_mut().unwrap() {
                    inner.as_object_mut().unwrap().remove("_aiHandoff");
                }
            }
        }
        let stripped = serde_json::to_string_pretty(&root).unwrap();

        let (second, _) = apply(Some(&stripped), exe).unwrap();
        let v: Value = serde_json::from_str(&second).unwrap();
        for (event, _) in MANAGED_EVENTS {
            assert_eq!(v["hooks"][event].as_array().unwrap().len(), 1, "{event}");
        }

        let cleaned = remove(&stripped).unwrap();
        let c: Value = serde_json::from_str(&cleaned).unwrap();
        assert!(c.get("hooks").is_none() || c["hooks"].as_object().unwrap().is_empty());
    }

    #[test]
    fn remove_does_not_insert_null_hooks_when_hooks_key_is_absent() {
        let cleaned = remove(r#"{"other":true}"#).unwrap();
        let c: Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(c["other"], true);
        assert!(c.get("hooks").is_none());
    }
}
