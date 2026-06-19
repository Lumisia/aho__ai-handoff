import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { projectDir } from '../core/lib/paths.mjs';
import { buildCapsule } from '../core/capsule/create.mjs';
import { publishCapsule } from '../core/capsule/store.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-index-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

function capsule(taskId, goal) {
  return buildCapsule({
    capsuleId: taskId, taskId, now: '2026-06-19T00:00:00Z',
    source: { agent: 'codex' }, target: { agent: 'claude-code' }, trigger: { type: 'test' },
    project: { fingerprint: 'fp' }, checkpoint: { status: 'in_progress' }, task: { goal },
  });
}

test('publishing refreshes machine manifest and thin INDEX pointers', () => withRoot(() => {
  const knowledge = join(projectDir('fp'), 'project');
  mkdirSync(knowledge, { recursive: true });
  writeFileSync(join(knowledge, 'gotchas.md'), 'one');
  publishCapsule('fp', capsule('t-a-aaaaaaaaaaaa', 'a'), { now: 1 });
  const manifestPath = join(projectDir('fp'), 'manifest.json');
  assert.equal(JSON.parse(readFileSync(manifestPath, 'utf8')).files['gotchas.md'].dirty, true);
  assert.match(readFileSync(join(projectDir('fp'), 'INDEX.md'), 'utf8'), /t-a-aaaaaaaaaaaa/);
  writeFileSync(join(knowledge, 'gotchas.md'), 'two');
  publishCapsule('fp', capsule('t-b-bbbbbbbbbbbb', 'b'), { now: 2 });
  assert.match(readFileSync(join(projectDir('fp'), 'INDEX.md'), 'utf8'), /gotchas\.md\s+\[MODIFIED\]/);
}));
