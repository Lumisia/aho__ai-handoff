export const MESSAGES = {
  en: {
    'ask.create_or_skip': 'Create a capsule? /handoff create | /handoff skip',
    'notify.capsule_ready': 'Capsule ready for {agent}',
    'notice.newer_pending': 'NEW HANDOFF PENDING — a newer capsule you have not pulled yet',
    'notice.newer_pending_action': '→ Run /handoff to pull this capsule into context. (taskId: {taskId})',
    'summary.instruction': 'Create the handoff capsule now. Reply with exactly one semantic summary wrapped in <handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule>. Do not include secrets, hidden reasoning, or transcript text.',
  },
  ko: {
    'ask.create_or_skip': '캡슐을 생성할까요? /handoff create | /handoff skip',
    'notify.capsule_ready': '{agent}에게 전달할 캡슐이 준비됨',
    'notice.newer_pending': '새 핸드오프 대기 중 — 아직 가져오지 않은 더 새로운 캡슐이 있습니다',
    'notice.newer_pending_action': '→ /handoff 를 실행해 이 캡슐을 컨텍스트로 가져오세요. (taskId: {taskId})',
    'summary.instruction': '지금 핸드오프 캡슐을 만드세요. <handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule> 형식의 의미 요약 하나만 답하세요. 비밀·숨은 추론·대화 원문은 포함하지 마세요.',
  },
  ja: {
    'ask.create_or_skip': 'カプセルを作成しますか？ /handoff create | /handoff skip',
    'notify.capsule_ready': '{agent} 向けのカプセルが準備できました',
    'notice.newer_pending': '新しいハンドオフが保留中 — まだ取り込んでいない新しいカプセルがあります',
    'notice.newer_pending_action': '→ /handoff を実行してこのカプセルをコンテキストに取り込んでください。(taskId: {taskId})',
    'summary.instruction': '今すぐハンドオフ・カプセルを作成してください。<handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule> 形式の意味要約を1つだけ返してください。秘密・隠れた推論・会話本文は含めないでください。',
  },
  zh: {
    'ask.create_or_skip': '创建胶囊吗？ /handoff create | /handoff skip',
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
