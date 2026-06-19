import { test } from 'node:test';
import assert from 'node:assert/strict';
import { renderIndex } from '../core/project/index-md.mjs';

test('renders changed entries and current task pointer', () => {
  const md = renderIndex({
    project: 'demo',
    changed: [{ path: 'format.md', status: 'MODIFIED' }, { path: 'gotchas.md', status: 'NEW' }],
    taskId: 't-x-aaaaaaaaaaaa',
  });
  assert.match(md, /# demo handoff index/);
  assert.match(md, /CHANGED SINCE LAST HANDOFF/);
  assert.match(md, /format\.md\s+\[MODIFIED\]/);
  assert.match(md, /handoff\/t-x-aaaaaaaaaaaa\/capsule\.json/);
});

test('empty changed list renders (none)', () => {
  const md = renderIndex({ changed: [] });
  assert.match(md, /- \(none\)/);
});
