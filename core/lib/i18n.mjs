export const MESSAGES = {
  en: {
    'ask.create_or_skip': 'Create a capsule? /handoff create | /handoff skip',
    'ask.instruct.claude': 'A handoff capsule is recommended now — do not decide for the user. Use the AskUserQuestion tool to ask exactly one question (header "Handoff capsule", question "Create a capsule?") with two options: "Yes" (recommended) → run /handoff create, and "No" → run /handoff skip. The tool adds a free-text "Other" option automatically; do not add your own. If the user picks Other, treat their text as capsule requirements: run /handoff create incorporating it, or ask once for only the missing detail if it is unclear. Do not run /handoff create or /handoff skip until the user has answered.',
    'ask.instruct.codex': 'A handoff capsule is recommended now — do not decide for the user. If the request_user_input tool is available, call it once with one question (header "Handoff capsule", question "Create a capsule?") and two options: "Yes" (recommended) → run /handoff create, and "No" → run /handoff skip; do not add an "Other" option, the client adds a free-text one automatically. If request_user_input is unavailable or refused, instead ask in one line of text "Create a capsule? Yes / No / Other (describe what you want)" and wait. On the answer: Yes → /handoff create, No → /handoff skip, Other → treat the text as capsule requirements (run /handoff create incorporating it, or ask once for only the missing detail if unclear). Do not run /handoff create or /handoff skip until the user has answered.',
    'notify.capsule_ready': 'Capsule ready for {agent}',
    'notice.newer_pending': 'NEW HANDOFF PENDING — a newer capsule you have not pulled yet',
    'notice.newer_pending_action': '→ Run /handoff to pull this capsule into context. (taskId: {taskId})',
    'summary.instruction': 'Create the handoff capsule now. Reply with exactly one semantic summary wrapped in <handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule>. Do not include secrets, hidden reasoning, or transcript text.',
  },
  ko: {
    'ask.create_or_skip': '캡슐을 생성할까요? /handoff create | /handoff skip',
    'ask.instruct.claude': '지금 핸드오프 캡슐 생성이 권장됩니다 — 사용자 대신 결정하지 마세요. AskUserQuestion 도구로 질문 하나만 띄우세요(header "핸드오프 캡슐", question "캡슐을 생성할까요?"): "네"(권장) → /handoff create 실행, "아니요" → /handoff skip 실행. 도구가 자유입력 "기타" 항목을 자동 추가하므로 직접 넣지 마세요. 사용자가 기타를 고르면 입력 내용을 캡슐 요구사항으로 보고 /handoff create에 반영하거나, 불명확하면 필요한 정보만 한 번 더 물으세요. 사용자가 답하기 전에는 /handoff create나 /handoff skip을 실행하지 마세요.',
    'ask.instruct.codex': '지금 핸드오프 캡슐 생성이 권장됩니다 — 사용자 대신 결정하지 마세요. request_user_input 도구를 사용할 수 있으면 질문 하나만 호출하세요(header "핸드오프 캡슐", question "캡슐을 생성할까요?"): "네"(권장) → /handoff create 실행, "아니요" → /handoff skip 실행. "기타" 항목은 직접 추가하지 마세요 — client가 자유입력 항목을 자동 추가합니다. request_user_input을 쓸 수 없거나 거절되면, 대신 한 줄로 "캡슐을 생성할까요? 네 / 아니요 / 기타(원하는 내용 입력)"라고 묻고 기다리세요. 답변 처리: 네 → /handoff create, 아니요 → /handoff skip, 기타 → 입력 내용을 캡슐 요구사항으로 반영(불명확하면 필요한 정보만 한 번 더 질문). 사용자가 답하기 전에는 /handoff create나 /handoff skip을 실행하지 마세요.',
    'notify.capsule_ready': '{agent}에게 전달할 캡슐이 준비됨',
    'notice.newer_pending': '새 핸드오프 대기 중 — 아직 가져오지 않은 더 새로운 캡슐이 있습니다',
    'notice.newer_pending_action': '→ /handoff 를 실행해 이 캡슐을 컨텍스트로 가져오세요. (taskId: {taskId})',
    'summary.instruction': '지금 핸드오프 캡슐을 만드세요. <handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule> 형식의 의미 요약 하나만 답하세요. 비밀·숨은 추론·대화 원문은 포함하지 마세요.',
  },
  ja: {
    'ask.create_or_skip': 'カプセルを作成しますか？ /handoff create | /handoff skip',
    'ask.instruct.claude': '今ハンドオフ・カプセルの作成が推奨されます — ユーザーの代わりに決定しないでください。AskUserQuestion ツールで質問を1つだけ表示してください（header「ハンドオフ・カプセル」、question「カプセルを作成しますか？」）：「はい」（推奨）→ /handoff create を実行、「いいえ」→ /handoff skip を実行。ツールが自由入力の「その他」を自動追加するので自分で追加しないでください。ユーザーが「その他」を選んだら、その内容をカプセルの要件として /handoff create に反映するか、不明確なら不足情報だけ一度確認してください。ユーザーが回答するまで /handoff create も /handoff skip も実行しないでください。',
    'ask.instruct.codex': '今ハンドオフ・カプセルの作成が推奨されます — ユーザーの代わりに決定しないでください。request_user_input ツールが利用可能なら、質問を1つだけ呼び出してください（header「ハンドオフ・カプセル」、question「カプセルを作成しますか？」）：「はい」（推奨）→ /handoff create、「いいえ」→ /handoff skip。「その他」項目は自分で追加しないでください — client が自由入力項目を自動追加します。request_user_input が利用できないか拒否された場合は、代わりに1行で「カプセルを作成しますか？ はい / いいえ / その他（希望内容を入力）」と尋ねて待ってください。回答の処理：はい → /handoff create、いいえ → /handoff skip、その他 → 内容をカプセル要件として反映（不明確なら不足情報だけ一度確認）。ユーザーが回答するまで /handoff create も /handoff skip も実行しないでください。',
    'notify.capsule_ready': '{agent} 向けのカプセルが準備できました',
    'notice.newer_pending': '新しいハンドオフが保留中 — まだ取り込んでいない新しいカプセルがあります',
    'notice.newer_pending_action': '→ /handoff を実行してこのカプセルをコンテキストに取り込んでください。(taskId: {taskId})',
    'summary.instruction': '今すぐハンドオフ・カプセルを作成してください。<handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule> 形式の意味要約を1つだけ返してください。秘密・隠れた推論・会話本文は含めないでください。',
  },
  zh: {
    'ask.create_or_skip': '创建胶囊吗？ /handoff create | /handoff skip',
    'ask.instruct.claude': '现在建议创建交接胶囊 — 不要替用户做决定。使用 AskUserQuestion 工具只问一个问题（header「交接胶囊」，question「创建胶囊吗？」）：「是」（推荐）→ 运行 /handoff create，「否」→ 运行 /handoff skip。工具会自动添加自由输入的「其他」选项，不要自行添加。若用户选择「其他」，将其内容作为胶囊需求并据此运行 /handoff create，若不清楚则只追问缺失的信息一次。在用户回答之前，不要运行 /handoff create 或 /handoff skip。',
    'ask.instruct.codex': '现在建议创建交接胶囊 — 不要替用户做决定。若 request_user_input 工具可用，只调用一次提出一个问题（header「交接胶囊」，question「创建胶囊吗？」）：「是」（推荐）→ /handoff create，「否」→ /handoff skip；不要添加「其他」选项，client 会自动添加自由输入项。若 request_user_input 不可用或被拒绝，则改为用一行询问「创建胶囊吗？ 是 / 否 / 其他（描述你的需求）」并等待。处理回答：是 → /handoff create，否 → /handoff skip，其他 → 将内容作为胶囊需求（不清楚则只追问缺失信息一次）。在用户回答之前，不要运行 /handoff create 或 /handoff skip。',
    'notify.capsule_ready': '已为 {agent} 准备好胶囊',
    'notice.newer_pending': '有新的交接待处理 — 存在你尚未拉取的更新胶囊',
    'notice.newer_pending_action': '→ 运行 /handoff 将该胶囊拉入上下文。(taskId: {taskId})',
    'summary.instruction': '现在创建交接胶囊。仅回复一个用 <handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule> 包裹的语义摘要。不要包含密钥、隐藏推理或对话原文。',
  },
};

export function t(key, vars = {}, locale = 'en') {
  const table = MESSAGES[locale] || MESSAGES.en;
  const template = table[key] ?? MESSAGES.en[key] ?? key;
  return template.replace(/\{(\w+)\}/g, (_, k) => (k in vars ? String(vars[k]) : `{${k}}`));
}

// The Stop-hook `ask` continuation prompt. `decision:block` feeds this to the
// MODEL (both Claude Code and Codex), so it is phrased as an instruction to ask
// the human via the harness picker — AskUserQuestion (Claude) or
// request_user_input (Codex, with a text fallback) — and to NOT decide. The
// human's choice drives the existing /handoff create | /handoff skip path.
export function askInstruction(agent, locale = 'en') {
  const key = agent === 'claude-code' ? 'ask.instruct.claude' : 'ask.instruct.codex';
  return t(key, {}, locale);
}
