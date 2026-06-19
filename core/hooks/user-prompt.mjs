import { readFileSync } from 'node:fs';
import { projectFingerprint } from '../lib/fingerprint.mjs';
import { gitContext } from '../lib/gitctx.mjs';
import { sha256OfJson } from '../lib/hash.mjs';
import { memoryRecallStatePath } from '../lib/paths.mjs';
import { writeFileAtomic, acquireLock, releaseLock } from '../lib/fsx.mjs';
import { readVerifiedShards } from '../memory/store.mjs';
import { rankMemoryShards, renderMemoryRecall } from '../memory/recall.mjs';

function readState() {
  try { return JSON.parse(readFileSync(memoryRecallStatePath(), 'utf8')); }
  catch { return { delivered: {} }; }
}

export function prepareUserPrompt({ input = {}, agent, tokenBudget = 800 }) {
  const cwd = input.cwd || process.cwd();
  const fingerprint = projectFingerprint(cwd);
  const key = sha256OfJson({ agent, session: input.session_id || 'unknown', fingerprint }).slice(0, 20);
  if (readState().delivered?.[key]) return { injected: false, reason: 'already-recalled' };
  const git = gitContext(cwd);
  const ranked = rankMemoryShards(readVerifiedShards(fingerprint), {
    prompt: input.prompt || '', branch: git.branch,
  });
  const context = renderMemoryRecall(ranked, { tokenBudget });
  if (!context) return { injected: false, reason: 'no-relevant-memory' };
  return { injected: true, context, delivery: { key } };
}

export function finalizeUserPrompt(delivery, { now = Date.now() } = {}) {
  const path = memoryRecallStatePath();
  const lock = acquireLock(`${path}.lock`, { now });
  if (!lock) throw new Error('memory recall state is locked');
  try {
    const state = readState();
    state.delivered[delivery.key] = now;
    writeFileAtomic(path, JSON.stringify(state, null, 2) + '\n');
  } finally { releaseLock(lock); }
}
