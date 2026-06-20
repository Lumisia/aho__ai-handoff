import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { readHistory } from '../core/capsule/history.mjs';
import { readSamples } from '../core/sensors/samples.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

function runWorkers(count, code, env) {
  return Promise.all(Array.from({ length: count }, (_, i) => new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ['--input-type=module', '-e', code], {
      env: { ...process.env, ...env, WORKER: String(i) },
    });
    let stderr = '';
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('exit', (codeOut) => (codeOut === 0 ? resolve() : reject(new Error(`worker ${i} exited ${codeOut}: ${stderr}`))));
  })));
}

test('concurrent appendHistory across processes loses no entries', async () => {
  const data = mkdtempSync(join(tmpdir(), 'ah-hist-'));
  process.env.AI_HANDOFF_ROOT = data; // main process must read the same store the workers wrote
  const histUrl = pathToFileURL(join(root, 'core', 'capsule', 'history.mjs')).href;
  const workers = 10;
  const perWorker = 20;
  const code = `
    import { appendHistory } from ${JSON.stringify(histUrl)};
    const w = process.env.WORKER;
    for (let s = 0; s < ${perWorker}; s++) appendHistory('testfp', { event: 'e', id: w + '-' + s });
  `;
  await runWorkers(workers, code, { AI_HANDOFF_ROOT: data });

  const entries = readHistory('testfp', { limit: 10000 });
  const ids = new Set(entries.map((e) => e.id));
  assert.equal(ids.size, workers * perWorker, `expected ${workers * perWorker} unique entries, got ${ids.size}`);
});

test('concurrent appendSample stays a valid bounded array', async () => {
  const data = mkdtempSync(join(tmpdir(), 'ah-samp-'));
  process.env.AI_HANDOFF_ROOT = data; // main process must read the same store the workers wrote
  const sampUrl = pathToFileURL(join(root, 'core', 'sensors', 'samples.mjs')).href;
  const code = `
    import { appendSample } from ${JSON.stringify(sampUrl)};
    const w = Number(process.env.WORKER);
    for (let s = 0; s < 10; s++) appendSample('testfp', 'claude-code', { usedPercent: w * 10 + s, at: Date.now() + s });
  `;
  await runWorkers(8, code, { AI_HANDOFF_ROOT: data });

  const samples = readSamples('testfp', 'claude-code');
  assert.ok(Array.isArray(samples), 'samples file remains a valid JSON array');
  assert.ok(samples.length <= 6, `ring buffer cap respected, got ${samples.length}`);
  assert.ok(samples.every((s) => typeof s.usedPercent === 'number'), 'no torn writes');
});
