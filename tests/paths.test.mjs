import { test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { dataRoot, configPath, projectDir, handoffDir } from '../core/lib/paths.mjs';

test('dataRoot honors AI_HANDOFF_ROOT override', () => {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = join('X:', 'ah');
  try {
    assert.equal(dataRoot(), join('X:', 'ah'));
    assert.equal(configPath(), join('X:', 'ah', 'config.json'));
    assert.equal(projectDir('fp1'), join('X:', 'ah', 'projects', 'fp1'));
    assert.equal(handoffDir('fp1'), join('X:', 'ah', 'projects', 'fp1', 'handoff'));
  } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
});
