export const INIT_REQUEST = {
  method: 'initialize',
  id: 0,
  params: { clientInfo: { name: 'ai-handoff', title: 'AI Handoff', version: '0.1.0' } },
};

export function extractPrimary(result) {
  const p = result?.rateLimits?.primary;
  if (!p || typeof p.usedPercent !== 'number') return null;
  return {
    usedPercent: p.usedPercent,
    windowMinutes: p.windowDurationMins,
    resetsAt: p.resetsAt,
    source: 'app-server',
  };
}

// 수신 메시지 1개에 대한 다음 동작을 반환. 부수효과 없음.
export function reduce(msg) {
  if (msg.id === 0 && msg.result !== undefined) {
    return {
      send: [
        { method: 'initialized', params: {} },
        { method: 'account/rateLimits/read', id: 1, params: {} },
      ],
    };
  }
  if (msg.id === 1 && msg.result !== undefined) {
    return { result: extractPrimary(msg.result) };
  }
  if (msg.id === 1 && msg.error !== undefined) {
    return { error: msg.error };
  }
  return {};
}
