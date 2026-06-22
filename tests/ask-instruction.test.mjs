import { test } from 'node:test';
import assert from 'node:assert/strict';
import { askInstruction, MESSAGES } from '../core/lib/i18n.mjs';

test('claude-code instruction drives the AskUserQuestion picker, not a decision', () => {
  const reason = askInstruction('claude-code', 'en');
  assert.match(reason, /AskUserQuestion/);
  assert.match(reason, /\/handoff create/);
  assert.match(reason, /\/handoff skip/);
  // The model must ASK, not decide for the user.
  assert.match(reason, /do not/i);
});

test('codex instruction prefers request_user_input and falls back to text', () => {
  const reason = askInstruction('codex', 'en');
  assert.match(reason, /request_user_input/);
  assert.match(reason, /unavailable/i); // text fallback path
  assert.match(reason, /\/handoff create/);
  assert.match(reason, /\/handoff skip/);
  assert.match(reason, /do not/i);
});

test('askInstruction selects the key by agent and localizes', () => {
  assert.equal(askInstruction('claude-code', 'ko'), MESSAGES.ko['ask.instruct.claude']);
  assert.equal(askInstruction('codex', 'ko'), MESSAGES.ko['ask.instruct.codex']);
  // Tool names stay literal across locales.
  assert.match(askInstruction('codex', 'ko'), /request_user_input/);
  assert.match(askInstruction('claude-code', 'ko'), /AskUserQuestion/);
});

test('unknown locale falls back to en', () => {
  assert.equal(askInstruction('codex', 'xx'), MESSAGES.en['ask.instruct.codex']);
  assert.equal(askInstruction('claude-code', 'xx'), MESSAGES.en['ask.instruct.claude']);
});

test('any non-claude agent uses the codex instruction', () => {
  assert.equal(askInstruction('codex', 'en'), MESSAGES.en['ask.instruct.codex']);
});
