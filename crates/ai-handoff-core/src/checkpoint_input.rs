use serde_json::{json, Map, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CheckpointInputError {
    #[error("invalid checkpoint JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("checkpoint JSON must be an object")]
    JsonMustBeObject,
    #[error(
        "checkpoint Markdown must include a Goal, Done, Remaining, Risks, or Next Prompt heading"
    )]
    MissingMarkdownSections,
}

pub fn parse_checkpoint_input(
    raw: &str,
    requested_format: Option<crate::config::CapsuleFormat>,
) -> Result<Value, CheckpointInputError> {
    let text = raw.trim();
    if text.is_empty() {
        return Ok(json!({}));
    }

    match requested_format {
        Some(crate::config::CapsuleFormat::Json) => parse_json_object(text),
        Some(crate::config::CapsuleFormat::Md) => parse_markdown(text),
        None => match serde_json::from_str::<Value>(text) {
            Ok(value) if value.is_object() => Ok(value),
            Ok(_) => Ok(json!({})),
            Err(_) => parse_markdown(text).or_else(|_| Ok(json!({}))),
        },
    }
}

fn parse_json_object(text: &str) -> Result<Value, CheckpointInputError> {
    let value = serde_json::from_str::<Value>(text)?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(CheckpointInputError::JsonMustBeObject)
    }
}

#[derive(Clone, Copy)]
enum Section {
    Goal,
    Done,
    Remaining,
    Risks,
    NextPrompt,
}

#[derive(Default)]
struct MarkdownSections {
    goal: Vec<String>,
    done: Vec<String>,
    remaining: Vec<String>,
    risks: Vec<String>,
    next_prompt: Vec<String>,
}

impl MarkdownSections {
    fn lines_mut(&mut self, section: Section) -> &mut Vec<String> {
        match section {
            Section::Goal => &mut self.goal,
            Section::Done => &mut self.done,
            Section::Remaining => &mut self.remaining,
            Section::Risks => &mut self.risks,
            Section::NextPrompt => &mut self.next_prompt,
        }
    }
}

fn parse_markdown(text: &str) -> Result<Value, CheckpointInputError> {
    let mut sections = MarkdownSections::default();
    let mut current = None;
    let mut found_section = false;
    let mut fenced = false;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.as_bytes().starts_with(b"```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
        }

        if !fenced {
            if let Some(title) = atx_heading(line) {
                current = heading_section(title);
                found_section |= current.is_some();
                continue;
            }
        }

        if let Some(section) = current {
            sections.lines_mut(section).push(line.to_string());
        }
    }

    if !found_section {
        return Err(CheckpointInputError::MissingMarkdownSections);
    }

    let goal = section_items(&sections.goal).join("\n\n");
    let done = section_items(&sections.done);
    let remaining = section_items(&sections.remaining);
    let risks = section_items(&sections.risks);
    let next_prompt = section_items(&sections.next_prompt).join("\n");

    let mut object = Map::new();
    if !goal.is_empty() {
        object.insert("goal".to_string(), json!(goal));
    }
    object.insert("done".to_string(), json!(done));
    object.insert("remaining".to_string(), json!(remaining));
    object.insert("risks".to_string(), json!(risks));
    if !next_prompt.is_empty() {
        object.insert("next_prompt".to_string(), json!(next_prompt));
    }
    Ok(Value::Object(object))
}

fn atx_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let hash_count = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&hash_count) {
        return None;
    }

    let rest = &trimmed[hash_count..];
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim().trim_end_matches('#').trim())
}

fn heading_section(title: &str) -> Option<Section> {
    let normalized = title
        .trim()
        .trim_matches(|ch| matches!(ch, '*' | '_'))
        .trim_end_matches([':', '：'])
        .trim()
        .to_lowercase();

    match normalized.as_str() {
        "goal" | "objective" | "목표" | "작업 목표" | "目的" | "目標" => {
            Some(Section::Goal)
        }
        "done" | "completed" | "completed work" | "완료" | "완료 항목" | "완료한 작업" | "完了"
        | "完了項目" => Some(Section::Done),
        "remaining" | "remaining work" | "next actions" | "남은 작업" | "미완료" | "다음 작업"
        | "残り" | "残作業" | "残りの作業" => Some(Section::Remaining),
        "risk" | "risks" | "open issues" | "위험" | "위험 요소" | "리스크" | "주의사항"
        | "リスク" | "注意点" => Some(Section::Risks),
        "next prompt" | "next_prompt" | "다음 프롬프트" | "다음 지시" | "次のプロンプト" => {
            Some(Section::NextPrompt)
        }
        _ => None,
    }
}

fn section_items(lines: &[String]) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            push_item(&mut items, &mut current);
            continue;
        }

        if let Some(item) = strip_list_marker(trimmed) {
            push_item(&mut items, &mut current);
            current.push_str(item);
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(trimmed);
        }
    }
    push_item(&mut items, &mut current);
    items
}

fn strip_list_marker(line: &str) -> Option<&str> {
    for prefix in ["- ", "* ", "+ "] {
        if let Some(item) = line.strip_prefix(prefix) {
            return Some(item.trim());
        }
    }

    let digit_count = line.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    let rest = &line[digit_count..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim())
}

fn push_item(items: &mut Vec<String>, current: &mut String) {
    let item = current.trim();
    if !item.is_empty() {
        items.push(item.to_string());
    }
    current.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CapsuleFormat;
    use serde_json::json;

    #[test]
    fn parses_korean_markdown_headings_and_lists() {
        let raw = r#"# AI Handoff Checkpoint

## 목표

설정값을 따르는 체크포인트를 만든다.

## 완료

- guidance 계약 작성
- MD 파서 테스트 작성

## 남은 작업

1. CLI 연결
2. Claude와 Codex 스킬 갱신

## 위험 요소

- 이전 CLI 문법 호환성

## 다음 프롬프트

- CLI 구현
- 전체 테스트 실행
"#;

        let parsed = parse_checkpoint_input(raw, Some(CapsuleFormat::Md)).unwrap();

        assert_eq!(parsed["goal"], "설정값을 따르는 체크포인트를 만든다.");
        assert_eq!(
            parsed["done"],
            json!(["guidance 계약 작성", "MD 파서 테스트 작성"])
        );
        assert_eq!(
            parsed["remaining"],
            json!(["CLI 연결", "Claude와 Codex 스킬 갱신"])
        );
        assert_eq!(parsed["risks"], json!(["이전 CLI 문법 호환성"]));
        assert_eq!(parsed["next_prompt"], "CLI 구현\n전체 테스트 실행");
    }

    #[test]
    fn parses_english_paragraph_continuations_and_numbered_items() {
        let raw = r#"## Goal
Preserve checkpoint settings across
all supported agents.

## Done
1) Added the contract
   with a continuation.
2. Added parser cases.

## Remaining
Wire the command into clap.

## Risks
No known blocker.

## Next Prompt
Implement the CLI without breaking
the legacy command.
"#;

        let parsed = parse_checkpoint_input(raw, Some(CapsuleFormat::Md)).unwrap();

        assert_eq!(
            parsed["goal"],
            "Preserve checkpoint settings across all supported agents."
        );
        assert_eq!(
            parsed["done"],
            json!([
                "Added the contract with a continuation.",
                "Added parser cases."
            ])
        );
        assert_eq!(parsed["remaining"], json!(["Wire the command into clap."]));
        assert_eq!(parsed["risks"], json!(["No known blocker."]));
        assert_eq!(
            parsed["next_prompt"],
            "Implement the CLI without breaking the legacy command."
        );
    }

    #[test]
    fn parses_japanese_heading_aliases() {
        let raw = "## 目標\n引き継ぎを保存する。\n\n## 完了\n- 解析\n\n## 残り\n- 実装\n\n## リスク\n- なし\n\n## 次のプロンプト\n- 続行\n";

        let parsed = parse_checkpoint_input(raw, Some(CapsuleFormat::Md)).unwrap();

        assert_eq!(parsed["goal"], "引き継ぎを保存する。");
        assert_eq!(parsed["done"], json!(["解析"]));
        assert_eq!(parsed["remaining"], json!(["実装"]));
        assert_eq!(parsed["risks"], json!(["なし"]));
        assert_eq!(parsed["next_prompt"], "続行");
    }

    #[test]
    fn explicit_formats_reject_the_wrong_input_shape() {
        assert!(parse_checkpoint_input("## Goal\nnot json", Some(CapsuleFormat::Json)).is_err());
        assert!(
            parse_checkpoint_input(r#"{"goal":"not markdown"}"#, Some(CapsuleFormat::Md)).is_err()
        );
        assert!(parse_checkpoint_input("# unrelated\ntext", Some(CapsuleFormat::Md)).is_err());
    }

    #[test]
    fn no_explicit_format_auto_detects_json_or_markdown() {
        let json_input =
            parse_checkpoint_input(r#"{"goal":"json goal","done":["one"]}"#, None).unwrap();
        assert_eq!(json_input["goal"], "json goal");

        let md_input = parse_checkpoint_input("## Goal\nmarkdown goal", None).unwrap();
        assert_eq!(md_input["goal"], "markdown goal");
    }

    #[test]
    fn automatic_format_keeps_legacy_nonstructured_input_compatible() {
        assert_eq!(
            parse_checkpoint_input("legacy free-form note", None).unwrap(),
            json!({})
        );
        assert_eq!(
            parse_checkpoint_input("[\"legacy list\"]", None).unwrap(),
            json!({})
        );
    }

    #[test]
    fn empty_input_remains_valid_for_message_only_checkpoints() {
        assert_eq!(
            parse_checkpoint_input("  \n", Some(CapsuleFormat::Json)).unwrap(),
            json!({})
        );
        assert_eq!(
            parse_checkpoint_input("", Some(CapsuleFormat::Md)).unwrap(),
            json!({})
        );
    }
}
