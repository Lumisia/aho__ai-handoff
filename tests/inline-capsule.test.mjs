import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  codexAskInstruction,
  codexAutoFooterInstruction,
  extractInlineCapsule,
  fallbackInlineSentinel,
} from '../core/lib/inline-capsule.mjs';

test('extractInlineCapsule parses fenced JSON and trims arrays', () => {
  const capsule = extractInlineCapsule(`
answer

\`\`\`ai-handoff-capsule
{
  "goal": " finish auth ",
  "completed": [" edit ", "", 3],
  "next_actions": [" run tests "],
  "open_issues": [],
  "status": ""
}
\`\`\`
`);

  assert.deepEqual(capsule, {
    goal: 'finish auth',
    completed: ['edit'],
    next_actions: ['run tests'],
    open_issues: [],
    status: 'in_progress',
  });
});

test('extractInlineCapsule rejects invalid or missing goal payloads', () => {
  assert.equal(extractInlineCapsule('no capsule'), null);
  assert.equal(extractInlineCapsule('```ai-handoff-capsule\n{}\n```'), null);
  assert.equal(extractInlineCapsule('```ai-handoff-capsule\nnot json\n```'), null);
});

test('codex auto footer instruction asks for visible fenced footer without secrets', () => {
  const instruction = codexAutoFooterInstruction({ usedPercent: 86, thresholdPercent: 80, locale: 'ko' });
  assert.match(instruction, /캡슐을 완성했습니다/);
  assert.match(instruction, /```ai-handoff-capsule/);
  assert.match(instruction, /비밀값/);
});

test('codex ask instruction uses request_user_input and keeps Other client-side', () => {
  const instruction = codexAskInstruction({ locale: 'ko' });
  assert.match(instruction, /request_user_input/);
  assert.match(instruction, /기타.*직접 넣지 마세요|직접 넣지 마세요.*기타/);
  assert.match(instruction, /\/handoff create/);
  assert.match(instruction, /\/handoff skip/);
});

test('fallback inline sentinel records degraded reason', () => {
  assert.deepEqual(fallbackInlineSentinel({
    reading: { usedPercent: 91 },
    threshold: 80,
  }), {
    goal: 'auto checkpoint at 91%',
    completed: [],
    next_actions: ['Review the previous assistant response and continue from the saved checkpoint.'],
    open_issues: ['The model did not emit an inline ai-handoff-capsule footer, so this capsule is degraded.'],
    status: 'in_progress',
    threshold_percent: 80,
  });
});
