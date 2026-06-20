import { join } from 'node:path';
import { readFileSync, mkdirSync } from 'node:fs';
import { writeFileAtomic } from '../lib/fsx.mjs';
import { projectDir } from '../lib/paths.mjs';

function historyPath(fingerprint) { return join(projectDir(fingerprint), 'history.jsonl'); }

function readLines(path) {
  try { return readFileSync(path, 'utf8').split('\n').filter((l) => l.trim()); }
  catch { return []; }
}

export function appendHistory(fingerprint, entry, { now = Date.now(), max = 500 } = {}) {
  const path = historyPath(fingerprint);
  try { mkdirSync(projectDir(fingerprint), { recursive: true }); } catch {}
  const lines = readLines(path);
  lines.push(JSON.stringify({ ts: now, ...entry }));
  writeFileAtomic(path, lines.slice(-max).join('\n') + '\n');
}

export function readHistory(fingerprint, { limit = 20 } = {}) {
  return readLines(historyPath(fingerprint))
    .slice(-limit)
    .map((l) => { try { return JSON.parse(l); } catch { return null; } })
    .filter(Boolean);
}
