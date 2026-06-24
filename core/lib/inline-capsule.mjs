const INLINE_CAPSULE_RE = /```ai-handoff-capsule\s*([\s\S]*?)```/i;

function stringArray(value) {
  if (!Array.isArray(value)) return [];
  return value
    .filter((item) => typeof item === 'string')
    .map((item) => item.trim())
    .filter(Boolean);
}

export function extractInlineCapsule(text) {
  const match = String(text || '').match(INLINE_CAPSULE_RE);
  if (!match) return null;

  try {
    const value = JSON.parse(match[1]);
    if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
    if (typeof value.goal !== 'string' || !value.goal.trim()) return null;

    return {
      goal: value.goal.trim(),
      completed: stringArray(value.completed),
      next_actions: stringArray(value.next_actions),
      open_issues: stringArray(value.open_issues),
      status: typeof value.status === 'string' && value.status.trim()
        ? value.status.trim()
        : 'in_progress',
    };
  } catch {
    return null;
  }
}

export function fallbackInlineSentinel({ reading, threshold } = {}) {
  const pct = typeof reading?.usedPercent === 'number'
    ? `${reading.usedPercent}%`
    : 'unknown usage';

  return {
    goal: `auto checkpoint at ${pct}`,
    completed: [],
    next_actions: [
      'Review the previous assistant response and continue from the saved checkpoint.',
    ],
    open_issues: [
      'The model did not emit an inline ai-handoff-capsule footer, so this capsule is degraded.',
    ],
    status: 'in_progress',
    threshold_percent: threshold,
  };
}

export function codexAutoFooterInstruction({
  usedPercent,
  thresholdPercent,
  locale = 'ko',
} = {}) {
  const pct = typeof usedPercent === 'number' ? `${usedPercent}%` : 'unknown';

  if (locale === 'ko') {
    return `
AI Handoff 알림:
현재 Codex 5시간 사용량이 ${pct}이고, 캡슐 생성 기준은 ${thresholdPercent}%입니다.

사용자 요청을 중단하지 말고 계속 해결하세요.
최종 답변 맨 아래에 반드시 아래 섹션을 추가하세요.

형식:

---

캡슐을 완성했습니다.

\`\`\`ai-handoff-capsule
{
  "goal": "현재 작업 목표를 한 문장으로 요약",
  "completed": ["이번 턴에서 완료한 일"],
  "next_actions": ["다음 도구가 이어서 할 일"],
  "open_issues": ["남은 문제나 확인할 점"],
  "status": "in_progress"
}
\`\`\`

주의:
- 비밀값, 토큰, 개인 정보, 숨은 추론은 포함하지 마세요.
- 사용자에게 필요한 최종 답변을 먼저 완료하고, 맨 아래에 캡슐 섹션을 붙이세요.
- 캡슐 섹션만 답하지 마세요.
- JSON은 parse 가능한 순수 JSON이어야 합니다. 주석, trailing comma, markdown 안쪽 설명을 넣지 마세요.
`.trim();
  }

  return `
AI Handoff notice:
Codex five-hour usage is ${pct}; the capsule threshold is ${thresholdPercent}%.

Continue the user's task normally.
At the very bottom of your final answer, append this section:

---

Capsule completed.

\`\`\`ai-handoff-capsule
{
  "goal": "one-sentence current goal",
  "completed": ["work completed in this turn"],
  "next_actions": ["next steps for the next agent"],
  "open_issues": ["remaining issues or checks"],
  "status": "in_progress"
}
\`\`\`

Do not include secrets, tokens, personal data, private reasoning, or raw transcript text.
Do not answer with only the capsule section.
The JSON must be valid JSON with no comments or trailing commas.
`.trim();
}

export function codexAskInstruction({ locale = 'ko' } = {}) {
  if (locale === 'ko') {
    return `
AI Handoff 알림:
현재 5시간 한도가 얼마 남지 않았습니다.
사용자에게 캡슐 생성 여부를 물어보고, 답변을 처리한 뒤 원래 사용자 요청을 계속 진행하세요.

request_user_input 도구를 사용할 수 있으면 정확히 한 번 호출하세요.

권장 질문:
- header: "핸드오프"
- question: "현재 5시간 한도가 얼마 남지 않았습니다. 캡슐을 만드시겠습니까?"
- options:
  - "네 (Recommended)"
  - "아니오"

중요:
- options 배열에 "기타" 또는 "Other"를 직접 넣지 마세요. Codex client가 자유입력 Other 항목을 자동 추가합니다.
- request_user_input을 사용할 수 없으면 한 줄로 질문하고 기다리세요:
  "현재 5시간 한도가 얼마 남지 않았습니다. 캡슐을 만드시겠습니까? 네 / 아니오 / 기타(원하는 내용 입력)"

답변 처리:
- 네: 현재 작업 목적, 완료한 일, 남은 일, 열린 이슈를 짧게 정리해 /handoff create를 실행하고, 원래 작업을 계속 진행하세요.
- 아니오: /handoff skip을 실행하고, 원래 작업을 계속 진행하세요.
- 기타/자유입력: 입력 내용을 캡슐 요구사항으로 반영해 /handoff create를 실행하고, 원래 작업을 계속 진행하세요.

사용자가 답하기 전에는 /handoff create나 /handoff skip을 실행하지 마세요.
캡슐 처리만 하고 턴을 끝내지 마세요. 반드시 원래 사용자 요청을 계속 진행하세요.
`.trim();
  }

  return `
AI Handoff notice:
The five-hour limit is getting low.
Ask the user whether to create a capsule, handle the answer, then continue the original task.

If request_user_input is available, call it exactly once:
- header: "Handoff"
- question: "The five-hour limit is getting low. Create a handoff capsule?"
- options:
  - "Yes (Recommended)"
  - "No"

Do not include "Other" in the options array; the Codex client adds a free-form Other option automatically.

If request_user_input is unavailable, ask in one line:
"The five-hour limit is getting low. Create a handoff capsule? Yes / No / Other"

Answer handling:
- Yes: summarize goal, completed work, next actions, and open issues; run /handoff create; continue the original task.
- No: run /handoff skip; continue the original task.
- Other/free-form: incorporate the user's text into the capsule requirements; run /handoff create; continue the original task.

Do not create or skip before the user answers.
Do not end the turn after only handling the capsule; continue the original task.
`.trim();
}
