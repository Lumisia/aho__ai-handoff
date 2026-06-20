import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { handleStop } from '../core/hooks/stop.mjs';
import { t } from '../core/lib/i18n.mjs';

test('ask notification body is localized to ko', async () => {
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-i18n-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-i18np-'));
  const config = {
    locale: 'ko',
    triggers: { five_hour: { enabled: true, threshold_percent: 50, mode: 'ask' } },
    notification: { method: 'terminal' },
  };
  let captured = '';
  const readSensor = async () => ({ usedPercent: 90, windowMinutes: 300, resetsAt: null });
  await handleStop({ input: { cwd, session_id: 's' }, config, readSensor, agent: 'codex', now: 1, notifyFn: (t2, b) => { captured = b; } });
  assert.match(captured, /캡슐을 생성할까요/);
  delete process.env.AI_HANDOFF_ROOT;
});

test('ask notification body is localized to en (fails before t() wiring)', async () => {
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-i18e-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-i18ep-'));
  const config = { locale: 'en', triggers: { five_hour: { enabled: true, threshold_percent: 50, mode: 'ask' } }, notification: { method: 'terminal' } };
  let captured = '';
  await handleStop({ input: { cwd, session_id: 's' }, config, readSensor: async () => ({ usedPercent: 90, windowMinutes: 300, resetsAt: null }), agent: 'codex', now: 1, notifyFn: (t2, b) => { captured = b; } });
  assert.equal(captured, t('ask.create_or_skip', {}, 'en'));
  delete process.env.AI_HANDOFF_ROOT;
});
