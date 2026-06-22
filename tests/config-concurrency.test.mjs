import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { readUserConfig, getAt } from '../core/lib/config-edit.mjs';

const modUrl = new URL('../core/lib/config-edit.mjs', import.meta.url).href;

// Each child process runs one config:set on its own key, so the writes genuinely
// race. Without the file lock the last writer would clobber the others.
function setInChild(cfgPath, key) {
  return new Promise((resolve, reject) => {
    const code = `import(${JSON.stringify(modUrl)})`
      + `.then(m => m.setConfigValue(${JSON.stringify(cfgPath)}, ${JSON.stringify(key)}, false))`
      + `.then(() => process.exit(0)).catch(e => { console.error(e); process.exit(1); });`;
    const child = spawn(process.execPath, ['-e', code], { stdio: ['ignore', 'ignore', 'inherit'] });
    child.on('exit', (c) => (c === 0 ? resolve() : reject(new Error(`child set failed for ${key}: ${c}`))));
  });
}

test('concurrent config:set on different keys preserves every change', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-cfg-'));
  const cfgPath = join(dir, 'config.json');
  const keys = [
    'triggers.five_hour.enabled',
    'triggers.five_hour.burn_rate.enabled',
    'capsule.completed_autocreate',
    'handoff.notify_newer_pending',
    'memory.auto_recall',
    'statusline.show_handoff',
  ];
  await Promise.all(keys.map((k) => setInChild(cfgPath, k)));
  const cfg = readUserConfig(cfgPath);
  for (const k of keys) assert.equal(getAt(cfg, k), false, `lost update for ${k}`);
});
