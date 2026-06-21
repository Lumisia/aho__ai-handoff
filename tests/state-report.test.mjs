import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { stateReport } from '../core/lib/state-report.mjs';
import { doctorFor } from '../core/hooks/handoff.mjs';
import { appendHistory } from '../core/capsule/history.mjs';
import { saveApproval } from '../core/capsule/approval.mjs';
import { writeState } from '../core/capsule/store.mjs';
import { globalStatePath } from '../core/lib/paths.mjs';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';

test('stateReport counts entries and sizes for the growing state files', () => {
  const data = mkdtempSync(join(tmpdir(), 'ah-state-'));
  process.env.AI_HANDOFF_ROOT = data;
  const cwd = mkdtempSync(join(tmpdir(), 'ah-statecwd-'));
  const fp = projectFingerprint(cwd);

  writeState(globalStatePath(), { seen: { a: 1, b: 2, c: 3 } });
  appendHistory(fp, { event: 'created' });
  appendHistory(fp, { event: 'resumed' });
  saveApproval({ fingerprint: fp, key: 'k1', context: {} });

  const report = stateReport(fp);
  const by = (n) => report.find((r) => r.name === n);
  assert.equal(by('dedupe').entries, 3);
  assert.equal(by('history').entries, 2);
  assert.equal(by('approval').entries, 1);
  assert.ok(by('dedupe').bytes > 0, 'reports byte size');
});

test('doctor output includes the state file report', () => {
  const data = mkdtempSync(join(tmpdir(), 'ah-state2-'));
  process.env.AI_HANDOFF_ROOT = data;
  const cwd = mkdtempSync(join(tmpdir(), 'ah-state2cwd-'));
  const doc = doctorFor(cwd);
  assert.ok(Array.isArray(doc.stateFiles), 'doctor reports stateFiles');
  assert.ok(doc.stateFiles.some((r) => r.name === 'history'));
  assert.ok(doc.stateFiles.every((r) => typeof r.bytes === 'number'));
});
