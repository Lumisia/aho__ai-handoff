import { randomUUID } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { memoryDir } from '../lib/paths.mjs';
import { writeFileAtomic } from '../lib/fsx.mjs';
import { redactJson } from '../lib/redact.mjs';
import { sha256OfJson } from '../lib/hash.mjs';
import { validate } from '../lib/validate.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const schema = JSON.parse(readFileSync(join(here, '..', '..', 'schemas', 'memory-shard.schema.json'), 'utf8'));

export function memoryPayloadHash(shard) {
  const { integrity, ...payload } = shard;
  return sha256OfJson(payload);
}

export function validateMemoryShard(shard) {
  const checked = validate(shard, schema);
  const integrity = shard?.integrity?.payload_sha256 === `sha256:${memoryPayloadHash(shard)}`;
  return { valid: checked.valid && integrity, errors: [...checked.errors, ...(integrity ? [] : ['integrity mismatch'])] };
}

export function buildMemoryShard({
  shardId = randomUUID(), fingerprint, fact, evidence = [], tags = [], paths = [], branch,
  verified = true, now = Date.now(),
}) {
  const clean = redactJson({ fact, evidence, tags, paths, branch }).value;
  const shard = {
    schema_version: '1.0.0', shard_id: shardId, created_at: new Date(now).toISOString(),
    project: { fingerprint }, fact: clean.fact, evidence: clean.evidence,
    tags: clean.tags, paths: clean.paths, verified,
  };
  if (clean.branch) shard.branch = clean.branch;
  shard.integrity = { payload_sha256: `sha256:${memoryPayloadHash(shard)}` };
  return shard;
}

export function storeMemoryShard(fingerprint, shard) {
  const checked = validateMemoryShard(shard);
  if (!checked.valid || shard.project?.fingerprint !== fingerprint) throw new Error('invalid memory shard');
  if (shard.verified !== true || shard.evidence.length === 0) throw new Error('memory shard must be verified by evidence');
  const path = join(memoryDir(fingerprint), `${shard.shard_id}.json`);
  const text = JSON.stringify(shard, null, 2) + '\n';
  if (existsSync(path)) {
    if (readFileSync(path, 'utf8') !== text) throw new Error(`memory shard already stored: ${shard.shard_id}`);
    return { path, shard };
  }
  writeFileAtomic(path, text);
  return { path, shard };
}

export function readVerifiedShards(fingerprint) {
  const dir = memoryDir(fingerprint);
  if (!existsSync(dir)) return [];
  const shards = [];
  for (const name of readdirSync(dir)) {
    if (!name.endsWith('.json')) continue;
    try {
      const shard = JSON.parse(readFileSync(join(dir, name), 'utf8'));
      if (shard.verified === true && shard.project?.fingerprint === fingerprint && validateMemoryShard(shard).valid) {
        shards.push(shard);
      }
    } catch {}
  }
  return shards;
}
