import { test } from 'node:test';
import assert from 'node:assert/strict';
import { additionalContextOutput, stopContinuationOutput } from '../core/lib/hook-output.mjs';

test('Claude Stop continuation uses hookSpecificOutput.additionalContext', () => {
  assert.deepEqual(stopContinuationOutput('claude-code', 'ask user'), {
    hookSpecificOutput: { hookEventName: 'Stop', additionalContext: 'ask user' },
  });
});

test('Codex Stop continuation is opt-in and otherwise continues silently', () => {
  assert.deepEqual(stopContinuationOutput('codex', 'ask user'), { continue: true });
  assert.deepEqual(stopContinuationOutput('codex', 'ask user', { allowCodex: true }), {
    decision: 'block', reason: 'ask user',
  });
});

test('any non-claude agent continues silently unless explicitly allowed', () => {
  assert.deepEqual(stopContinuationOutput('whatever', 'go on'), { continue: true });
  assert.deepEqual(stopContinuationOutput('whatever', 'go on', { allowCodex: true }), {
    decision: 'block', reason: 'go on',
  });
});

test('empty or whitespace continuation lets the stop proceed', () => {
  assert.deepEqual(stopContinuationOutput('claude-code', ''), { continue: true });
  assert.deepEqual(stopContinuationOutput('codex', '   '), { continue: true });
  assert.deepEqual(stopContinuationOutput('claude-code', null), { continue: true });
});

test('additional context output uses hookSpecificOutput', () => {
  assert.deepEqual(additionalContextOutput('PostToolUse', 'inline capsule'), {
    hookSpecificOutput: {
      hookEventName: 'PostToolUse',
      additionalContext: 'inline capsule',
    },
  });
  assert.deepEqual(additionalContextOutput('UserPromptSubmit', '   '), { continue: true });
});
