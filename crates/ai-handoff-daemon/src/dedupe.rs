use ai_handoff_core::hook_event::HookEventKind;
use ai_handoff_ipc::protocol::Request;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};

pub fn dedupe_key(req: &Request) -> String {
    let mut hasher = Sha256::new();
    hasher.update(req.agent.as_bytes());
    hasher.update(b"\0");
    hasher.update(req.event.as_bytes());
    hasher.update(b"\0");
    hasher.update(req.session_id.as_deref().unwrap_or("").as_bytes());
    hasher.update(b"\0");
    hasher.update(req.turn_id.as_deref().unwrap_or("").as_bytes());
    // Multiple tools can finish in one turn. Claude can also omit turn_id
    // entirely. Neither case makes two distinct hook deliveries duplicates.
    let occurrence = match HookEventKind::parse(&req.event) {
        Some(HookEventKind::PostToolUse) => req
            .raw_hook_input
            .get("tool_use_id")
            .and_then(Value::as_str)
            .or(Some(req.request_id.as_str())),
        // Replayed Stop hooks must not write a second copy of a fenced capsule.
        // A later Stop in the same turn may now contain the requested capsule.
        Some(HookEventKind::Stop) => fenced_capsule_text(&req.raw_hook_input),
        _ => None,
    };
    if let Some(id) =
        occurrence.or_else(|| req.turn_id.is_none().then_some(req.request_id.as_str()))
    {
        hasher.update(b"\0");
        hasher.update(id.as_bytes());
    }
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Capsule text following the host's fenced block marker, when present.
pub(crate) fn fenced_capsule_text(raw: &Value) -> Option<&str> {
    let text = [
        "last_assistant_message",
        "final_answer",
        "message",
        "content",
    ]
    .iter()
    .find_map(|key| raw.get(*key).and_then(Value::as_str))?;
    let (_, block) = text.split_once("```ai-handoff-capsule")?;
    let (body, _) = block.split_once("```")?;
    Some(
        body.split_once('\n')
            .map_or(body, |(_, contents)| contents)
            .trim(),
    )
}

pub struct Deduper {
    seen: HashSet<String>,
    order: VecDeque<String>,
    cap: usize,
}

impl Deduper {
    pub fn new(cap: usize) -> Self {
        Self {
            seen: HashSet::new(),
            order: VecDeque::new(),
            cap,
        }
    }

    pub fn check_and_record(&mut self, key: &str) -> bool {
        if self.seen.contains(key) {
            return true;
        }
        if self.cap == 0 {
            return false;
        }

        let key = key.to_string();
        self.seen.insert(key.clone());
        self.order.push_back(key);

        while self.order.len() > self.cap {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_handoff_ipc::protocol::{ClientInfo, Request, VERSION};
    use serde_json::json;

    fn request(session_id: Option<&str>, turn_id: Option<&str>) -> Request {
        Request {
            version: VERSION,
            request_id: format!("req-{}", turn_id.unwrap_or("none")),
            kind: "hook_event".into(),
            agent: "codex".into(),
            event: "stop".into(),
            received_at: "2026-06-25T12:34:56Z".into(),
            cwd: "/repo".into(),
            session_id: session_id.map(str::to_owned),
            turn_id: turn_id.map(str::to_owned),
            raw_hook_input: json!({}),
            client: ClientInfo {
                binary_version: "2.0.0-mvp".into(),
                pid: 1,
                platform: "windows".into(),
            },
        }
    }

    #[test]
    fn key_is_stable_and_distinguishes_turns() {
        let a = dedupe_key(&request(Some("s"), Some("t1")));
        let b = dedupe_key(&request(Some("s"), Some("t1")));
        let c = dedupe_key(&request(Some("s"), Some("t2")));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn tools_in_the_same_turn_and_claude_turns_without_turn_ids_are_distinct() {
        let mut tool = request(Some("s"), Some("same-turn"));
        tool.event = "post-tool-use".into();
        tool.raw_hook_input = json!({"tool_use_id":"use-1", "tool_name":"Bash"});
        let mut answer = tool.clone();
        answer.raw_hook_input = json!({"tool_use_id":"use-2", "tool_name":"AskUserQuestion"});
        assert_ne!(dedupe_key(&tool), dedupe_key(&answer));
        let first = request(Some("s"), None);
        let mut later = first.clone();
        later.request_id = "next-hook".into();
        assert_ne!(dedupe_key(&first), dedupe_key(&later));
        let mut fenced = later.clone();
        fenced.raw_hook_input =
            json!({"last_assistant_message":"```ai-handoff-capsule\n{\"goal\":\"demo\"}\n```"});
        assert_ne!(dedupe_key(&later), dedupe_key(&fenced));
        let mut replay = fenced.clone();
        replay.request_id = "redelivery".into();
        assert_eq!(dedupe_key(&fenced), dedupe_key(&replay));
    }

    #[test]
    fn check_and_record_detects_duplicates() {
        let mut d = Deduper::new(10);
        assert!(!d.check_and_record("a"));
        assert!(d.check_and_record("a"));
        assert!(!d.check_and_record("b"));
    }

    #[test]
    fn evicts_oldest_beyond_cap() {
        let mut d = Deduper::new(2);
        assert!(!d.check_and_record("a"));
        assert!(!d.check_and_record("b"));
        assert!(!d.check_and_record("c"));
        assert!(!d.check_and_record("a"));
        assert!(d.check_and_record("c"));
    }
}
