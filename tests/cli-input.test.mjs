import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const cli = join(here, '..', 'core', 'cli.mjs');

function run(args, input, env) {
  return execFileSync(process.execPath, [cli, ...args], { input, encoding: 'utf8', env: { ...process.env, ...env } });
}

function freshProject() {
  const root = mkdtempSync(join(tmpdir(), 'ah-in-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const env = { AI_HANDOFF_ROOT: root };
  run(['handoff:checkpoint', '--agent', 'codex'], JSON.stringify({
    cwd, session_id: 's', sentinel: { goal: 'g', next_actions: ['n'] },
  }), env);
  return { cwd, env };
}

test('readInput strips a leading UTF-8 BOM from stdin', () => {
  const { cwd, env } = freshProject();
  const status = JSON.parse(run(['handoff:status'], '﻿' + JSON.stringify({ cwd }), env));
  assert.equal(status.pending, true);
});

test('--cwd flag supplies cwd with no stdin (argv keeps the path literal)', () => {
  const { cwd, env } = freshProject();
  const status = JSON.parse(run(['handoff:status', '--cwd', cwd], '', env));
  assert.equal(status.pending, true);
});

test('--cwd overrides a cwd present in stdin JSON', () => {
  const { cwd, env } = freshProject();
  const other = mkdtempSync(join(tmpdir(), 'ah-other-'));
  // stdin points at an empty project, --cwd points at the seeded one
  const status = JSON.parse(run(['handoff:status', '--cwd', cwd], JSON.stringify({ cwd: other }), env));
  assert.equal(status.pending, true);
});

test('--input <file> is read instead of stdin', () => {
  const { cwd, env } = freshProject();
  const file = join(mkdtempSync(join(tmpdir(), 'ah-input-')), 'in.json');
  writeFileSync(file, JSON.stringify({ cwd }), 'utf8');
  const status = JSON.parse(run(['handoff:status', '--input', file], '', env));
  assert.equal(status.pending, true);
});

test('--input file with a leading BOM still parses', () => {
  const { cwd, env } = freshProject();
  const file = join(mkdtempSync(join(tmpdir(), 'ah-input-')), 'in.json');
  writeFileSync(file, '﻿' + JSON.stringify({ cwd }), 'utf8');
  const status = JSON.parse(run(['handoff:status', '--input', file], '', env));
  assert.equal(status.pending, true);
});
