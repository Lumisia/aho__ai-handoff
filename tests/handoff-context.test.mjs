import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { changedFiles } from '../core/lib/gitctx.mjs';
import { buildCheckpointCapsule } from '../core/capsule/checkpoint.mjs';
import { publishCapsule } from '../core/capsule/store.mjs';
import { prepareSessionStart } from '../core/hooks/session-start.mjs';

function gitRepo() {
  const dir = mkdtempSync(join(tmpdir(), 'ah-ctx-'));
  const g = (args) => execFileSync('git', ['-C', dir, ...args], { stdio: 'ignore' });
  g(['init', '-q']);
  g(['config', 'user.email', 't@t']);
  g(['config', 'user.name', 't']);
  writeFileSync(join(dir, 'a.txt'), 'one\n');
  g(['add', '.']);
  g(['commit', '-q', '-m', 'init']);
  return { dir, g };
}

test('changedFiles lists working-tree changes versus HEAD', () => {
  const { dir } = gitRepo();
  assert.deepEqual(changedFiles(dir), [], 'clean tree has no changes');
  writeFileSync(join(dir, 'a.txt'), 'two\n');
  assert.deepEqual(changedFiles(dir), ['a.txt']);
});

test('changedFiles returns [] outside a git repo', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-nogit-'));
  assert.deepEqual(changedFiles(dir), []);
});

test('changedFiles includes untracked files', () => {
  const { dir } = gitRepo();
  writeFileSync(join(dir, 'new-untracked.txt'), 'x\n');
  assert.deepEqual(changedFiles(dir), ['new-untracked.txt']);
});

test('changedFiles merges tracked edits and untracked files without duplicates', () => {
  const { dir } = gitRepo();
  writeFileSync(join(dir, 'a.txt'), 'two\n'); // tracked edit
  writeFileSync(join(dir, 'b.txt'), 'new\n'); // untracked
  assert.deepEqual(changedFiles(dir).sort(), ['a.txt', 'b.txt']);
});

test('changedFiles caps the list and appends a truncation marker', () => {
  const { dir } = gitRepo();
  for (let i = 0; i < 5; i++) writeFileSync(join(dir, `f${i}.txt`), 'x\n');
  const out = changedFiles(dir, { max: 3 });
  assert.equal(out.length, 4, 'three files plus one marker');
  assert.match(out[3], /\+2 more/);
});

test('session-start injection carries completed, open_issues and changed_files', () => {
  const data = mkdtempSync(join(tmpdir(), 'ah-ctxdata-'));
  process.env.AI_HANDOFF_ROOT = data;
  const { dir } = gitRepo();
  writeFileSync(join(dir, 'a.txt'), 'changed\n'); // working-tree change -> changed_files

  const { capsule, fingerprint } = buildCheckpointCapsule({
    sentinel: {
      goal: 'finish the parser',
      next_actions: ['wire the lexer'],
      completed: ['tokenizer done'],
      open_issues: ['unicode edge case'],
      status: 'in_progress',
    },
    cwd: dir,
    agent: 'codex',
    sessionId: 'sess-ctx',
  });
  publishCapsule(fingerprint, capsule, { status: 'AVAILABLE' });

  const result = prepareSessionStart({ input: { cwd: dir, session_id: 'claude-x' }, agent: 'claude-code' });
  assert.equal(result.injected, true);
  const ctx = result.context;
  assert.match(ctx, /completed: tokenizer done/);
  assert.match(ctx, /open_issues: unicode edge case/);
  assert.match(ctx, /changed_files: .*a\.txt/);
});
