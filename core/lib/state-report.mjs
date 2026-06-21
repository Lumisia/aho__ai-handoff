import { statSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  dataRoot, globalStatePath, memoryRecallStatePath, projectDir, handoffDir,
} from './paths.mjs';

function sizeOf(path) {
  try { return statSync(path).size; } catch { return 0; }
}

function countKeys(path, topKey) {
  try {
    const value = JSON.parse(readFileSync(path, 'utf8'));
    const obj = topKey ? value[topKey] : value;
    return obj && typeof obj === 'object' ? Object.keys(obj).length : 0;
  } catch { return 0; }
}

function countLines(path) {
  try { return readFileSync(path, 'utf8').split('\n').filter((l) => l.trim()).length; }
  catch { return 0; }
}

// Read-only size/entry snapshot of the state files that accumulate over time.
// Surfaced by `/handoff doctor` so growth is visible; automatic pruning is a
// separate, deferred change.
export function stateReport(fingerprint) {
  const dedupe = globalStatePath();
  const generation = join(dataRoot(), 'generation-state.json');
  const recall = memoryRecallStatePath();
  const approval = join(handoffDir(fingerprint), 'approval-state.json');
  const history = join(projectDir(fingerprint), 'history.jsonl');
  return [
    { name: 'dedupe', scope: 'global', bytes: sizeOf(dedupe), entries: countKeys(dedupe, 'seen') },
    { name: 'generation', scope: 'global', bytes: sizeOf(generation), entries: countKeys(generation, 'generations') },
    { name: 'memory-recall', scope: 'global', bytes: sizeOf(recall), entries: null },
    { name: 'approval', scope: 'project', bytes: sizeOf(approval), entries: countKeys(approval, 'approvals') },
    { name: 'history', scope: 'project', bytes: sizeOf(history), entries: countLines(history) },
  ];
}
