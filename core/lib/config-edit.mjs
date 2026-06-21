import { readFileSync } from 'node:fs';
import { writeFileAtomic } from './fsx.mjs';

// Allowlist of user-settable keys. Editing the file by hand can set anything,
// but `config:set` is guided: only these keys, with type/range/enum checks, so
// a typo can't silently write a config the runtime ignores.
export const CONFIG_KEYS = {
  'triggers.five_hour.enabled': { type: 'boolean' },
  'triggers.five_hour.threshold_percent': { type: 'number', min: 1, max: 100 },
  'triggers.five_hour.mode': { type: 'enum', values: ['auto', 'ask', 'off'] },
  'triggers.five_hour.burn_rate.enabled': { type: 'boolean' },
  'triggers.five_hour.burn_rate.runway_minutes': { type: 'number', min: 5, max: 120 },
  'capsule.completed_autocreate': { type: 'boolean' },
  'handoff.notify_newer_pending': { type: 'boolean' },
  'notification.method': { type: 'enum', values: ['os', 'terminal', 'off'] },
  'notification.fallback': { type: 'enum', values: ['terminal', 'off'] },
  'memory.auto_recall': { type: 'boolean' },
  'memory.auto_recall_token_budget': { type: 'number', min: 1 },
  'statusline.show_handoff': { type: 'boolean' },
  'locale': { type: 'enum', values: ['en', 'ko', 'ja', 'zh'] },
};

function isObject(v) { return v && typeof v === 'object' && !Array.isArray(v); }
function clone(v) { return isObject(v) ? JSON.parse(JSON.stringify(v)) : {}; }

export function knownKeys() { return Object.keys(CONFIG_KEYS); }

// Accepts already-typed JSON values and forgiving strings ("80", "true").
export function validateKeyValue(key, raw) {
  const spec = CONFIG_KEYS[key];
  if (!spec) throw new Error(`unknown config key: ${key}. known keys: ${knownKeys().join(', ')}`);
  let v = raw;
  if (spec.type === 'boolean') {
    if (v === 'true') v = true; else if (v === 'false') v = false;
    if (typeof v !== 'boolean') throw new Error(`${key} expects true or false`);
  } else if (spec.type === 'number') {
    if (typeof v === 'string' && v.trim() !== '' && !Number.isNaN(Number(v))) v = Number(v);
    if (typeof v !== 'number' || Number.isNaN(v)) throw new Error(`${key} expects a number`);
    if (spec.min != null && v < spec.min) throw new Error(`${key} must be >= ${spec.min}`);
    if (spec.max != null && v > spec.max) throw new Error(`${key} must be <= ${spec.max}`);
  } else if (spec.type === 'enum' && !spec.values.includes(v)) {
    throw new Error(`${key} must be one of: ${spec.values.join(', ')}`);
  }
  return v;
}

export function getAt(obj, key) {
  return key.split('.').reduce((o, k) => (isObject(o) ? o[k] : undefined), obj);
}

export function setAt(obj, key, value) {
  const root = clone(obj);
  const parts = key.split('.');
  let cur = root;
  for (let i = 0; i < parts.length - 1; i++) {
    if (!isObject(cur[parts[i]])) cur[parts[i]] = {};
    cur = cur[parts[i]];
  }
  cur[parts[parts.length - 1]] = value;
  return root;
}

export function unsetAt(obj, key) {
  const root = clone(obj);
  const parts = key.split('.');
  let cur = root;
  for (let i = 0; i < parts.length - 1; i++) {
    if (!isObject(cur[parts[i]])) return root;
    cur = cur[parts[i]];
  }
  delete cur[parts[parts.length - 1]];
  return root;
}

export function readUserConfig(path) {
  try { return JSON.parse(readFileSync(path, 'utf8')); } catch { return {}; }
}

export function writeUserConfig(path, obj) {
  writeFileAtomic(path, JSON.stringify(obj, null, 2) + '\n');
}

export function setConfigValue(path, key, rawValue) {
  const value = validateKeyValue(key, rawValue);
  writeUserConfig(path, setAt(readUserConfig(path), key, value));
  return { key, value };
}

export function unsetConfigValue(path, key) {
  if (!CONFIG_KEYS[key]) throw new Error(`unknown config key: ${key}. known keys: ${knownKeys().join(', ')}`);
  writeUserConfig(path, unsetAt(readUserConfig(path), key));
  return { key };
}
