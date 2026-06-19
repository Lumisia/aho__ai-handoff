import { join } from 'node:path';
import { readFileSync } from 'node:fs';
import { dataRoot } from '../lib/paths.mjs';
import { sha256OfJson } from '../lib/hash.mjs';
import { writeFileAtomic, acquireLock, releaseLock } from '../lib/fsx.mjs';

const path = () => join(dataRoot(), 'generation-state.json');

function read() {
  try { return JSON.parse(readFileSync(path(), 'utf8')); }
  catch { return { generations: {} }; }
}

function mutate(fn, now) {
  const lock = acquireLock(`${path()}.lock`, { now });
  if (!lock) throw new Error('generation state is locked');
  try {
    const state = read();
    const result = fn(state);
    writeFileAtomic(path(), JSON.stringify(state, null, 2) + '\n');
    return result;
  } finally { releaseLock(lock); }
}

export function generationSlotKey({ agent, sessionId, projectFingerprint }) {
  return sha256OfJson({ agent, sessionId, projectFingerprint }).slice(0, 16);
}

export function saveGeneration({ slotKey, context, now = Date.now() }) {
  return mutate((state) => {
    const value = { slotKey, status: 'GENERATING', context, updated_at: now };
    state.generations[slotKey] = value;
    return value;
  }, now);
}

export function findGeneration(slotKey) {
  const value = read().generations?.[slotKey];
  return value?.status === 'GENERATING' ? value : null;
}

export function finishGeneration(slotKey, { now = Date.now() } = {}) {
  return mutate((state) => {
    const value = state.generations?.[slotKey];
    if (!value) return null;
    state.generations[slotKey] = { ...value, status: 'FINISHED', updated_at: now };
    return state.generations[slotKey];
  }, now);
}
