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
  };
  capsule.integrity = { payload_sha256: 'sha256:' + capsulePayloadHash(capsule) };
  return capsule;
}

export function validateCapsule(capsule) {
  return validate(capsule, SCHEMA);
}
