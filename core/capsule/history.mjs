import { join } from 'node:path';
import { readFileSync, mkdirSync, appendFileSync } from 'node:fs';
import { writeFileAtomic, withLock, sleepSync } from '../lib/fsx.mjs';
import { projectDir } from '../lib/paths.mjs';

function historyPath(fingerprint) { return join(projectDir(fingerprint), 'history.jsonl'); }

function readLines(path) {
  try { return readFileSync(path, 'utf8').split('\n').filter((l) => l.trim()); }
  catch { return []; }
}

export function appendHistory(fingerprint, entry, { now = Date.now(), max = 500 } = {}) {
  const path = historyPath(fingerprint);
  try { mkdirSync(projectDir(fingerprint), { recursive: true }); } catch {}
  const line = JSON.stringify({ ts: now, ...entry }) + '\n';
  // A pure append never reads existing lines, so it cannot clobber a write that
  // landed between a read and a rewrite — it needs no lock and always lands, so
  // no entry is ever lost even when the trim below is skipped. Retry only
  // transient Windows fs errors (AV/indexer briefly locking the file); never
  // throw — appendHistory runs on the unguarded publishCapsule path.
  for (let i = 0; i < 20; i++) {
    try { appendFileSync(path, line); break; }
    catch (error) {
      if (i === 19) { process.stderr.write(`[handoff] history append failed: ${error.message}\n`); return; }
      sleepSync(10);
    }
  }
  // The trim is a read-modify-write, so it must hold the lock to avoid clobbering
  // a concurrent append. If the lock can't be acquired we skip the trim; the cap
  // self-heals on the next append.
  withLock(`${path}.lock`, () => {
    try {
      const lines = readLines(path);
      if (lines.length > max) writeFileAtomic(path, lines.slice(-max).join('\n') + '\n');
    } catch { /* best-effort cap; self-heals on next append */ }
  });
}

export function readHistory(fingerprint, { limit = 20 } = {}) {
  return readLines(historyPath(fingerprint))
    .slice(-limit)
    .map((l) => { try { return JSON.parse(l); } catch { return null; } })
    .filter(Boolean);
}
