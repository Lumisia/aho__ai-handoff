import { randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { sha256OfJson } from '../lib/hash.mjs';
import { validate } from '../lib/validate.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const SCHEMA = JSON.parse(readFileSync(join(here, '..', '..', 'schemas', 'capsule.schema.json'), 'utf8'));

export function capsulePayloadHash(capsule) {
  const { integrity, ...payload } = capsule;
  return sha256OfJson(payload);
}

export function buildCapsule(input) {
  const capsule = {
    schema_version: '1.0.0',
    capsule_id: input.capsuleId || randomUUID(),
    task_id: input.taskId,
    created_at: input.now || new Date().toISOString(),
    source: input.source,
    target: input.target,
    trigger: input.trigger,
    project: input.project,
    checkpoint: input.checkpoint,
    task: input.task,
    security: input.security,
  };
  if (input.expiresAt) capsule.expires_at = input.expiresAt;
  capsule.integrity = { payload_sha256: 'sha256:' + capsulePayloadHash(capsule) };
  return capsule;
}

// Hard ceiling on the serialized capsule. The per-field schema bounds cap
// individual strings/lists; this guards the total so a model error or local
// tampering cannot grow disk, verification, and prompt-injection size without
// limit. Enforced wherever validateCapsule runs — both publish and the
// injection-time verify path.
const MAX_CAPSULE_BYTES = 128 * 1024;

export function validateCapsule(capsule) {
  const checked = validate(capsule, SCHEMA);
  const errors = [...checked.errors];
  const allowedAgents = new Set(['codex', 'claude-code']);
  let size = 0;
  try { size = Buffer.byteLength(JSON.stringify(capsule), 'utf8'); } catch { size = Infinity; }
  if (size > MAX_CAPSULE_BYTES) errors.push(`$: capsule serialized size ${size} exceeds ${MAX_CAPSULE_BYTES} bytes`);
  if (!String(capsule?.task?.goal || '').trim()) errors.push('$.task.goal: must be non-empty');
  if (!allowedAgents.has(capsule?.source?.agent)) errors.push('$.source.agent: unsupported agent');
  if (!allowedAgents.has(capsule?.target?.agent)) errors.push('$.target.agent: unsupported agent');
  if (capsule?.source?.agent === capsule?.target?.agent) errors.push('$: source and target agents must differ');
  return { valid: errors.length === 0, errors };
}
