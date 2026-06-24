import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { handleStop } from '../core/hooks/stop.mjs';
import { appendSample } from '../core/sensors/samples.mjs';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';

test('handleStop fires on burn-rate below threshold when enabled', async () => {
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-br-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-brp-'));
  const fp = projectFingerprint(cwd);
  const now = 1000 * 60000;
  appendSample(fp, 'claude-code', { usedPercent: 60, at: now - 10 * 60000 });
  const config = {
    triggers: { five_hour: { enabled: true, threshold_percent: 95, mode: 'ask', burn_rate: { enabled: true, runway_minutes: 30 } } },
    notification: { method: 'off' },
  };
  const readSensor = async () => ({ usedPercent: 80, windowMinutes: 300, resetsAt: null });
  const res = await handleStop({ input: { cwd, session_id: 's' }, config, readSensor, agent: 'claude-code', now, notifyFn: () => {} });
  assert.equal(res.action, 'ask');
  assert.equal(res.reason, 'burn-rate');
  delete process.env.AI_HANDOFF_ROOT;
});
