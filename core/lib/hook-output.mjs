export function continueOutput() {
  return { continue: true };
}

export function additionalContextOutput(eventName, text) {
  const message = String(text || '').trim();
  if (!message) return continueOutput();
  return {
    hookSpecificOutput: {
      hookEventName: eventName,
      additionalContext: message,
    },
  };
}

// Stop-hook continuation output, shaped per agent. Codex `decision:"block"` is
// a visible continuation prompt, so it is opt-in only.
export function stopContinuationOutput(agent, text, { allowCodex = false } = {}) {
  const message = String(text || '').trim();
  if (!message) return continueOutput();
  if (agent === 'claude-code') {
    return { hookSpecificOutput: { hookEventName: 'Stop', additionalContext: message } };
  }
  if (!allowCodex) return continueOutput();
  return { decision: 'block', reason: message };
}
