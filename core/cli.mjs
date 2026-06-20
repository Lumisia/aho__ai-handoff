import { join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { codexHome, newestSessionFile } from './lib/sessions.mjs';
import { readJsonlRateLimit } from './sensors/codex-jsonl.mjs';
import { readAppServerRateLimit } from './sensors/codex-appserver.mjs';
import { readRateLimit } from './sensors/ratelimit.mjs';
import { recordClaudeRateLimit, readClaudeRateLimit } from './sensors/claude-statusline.mjs';
import { handleStop } from './hooks/stop.mjs';
import {
  prepareSessionStart, finalizeSessionStart, abortSessionStart,
} from './hooks/session-start.mjs';
import { loadConfig } from './lib/config.mjs';
import { configPath } from './lib/paths.mjs';
import {
  setConfigValue, unsetConfigValue, getAt, knownKeys,
} from './lib/config-edit.mjs';
import {
  statusFor, previewFor, createFromApproval, skipApproval, recoverFor,
} from './hooks/handoff.mjs';
import { buildCheckpointCapsule } from './capsule/checkpoint.mjs';
import { publishCapsule } from './capsule/store.mjs';
import {
  installClaudeStatusline, restoreClaudeStatusline, defaultClaudeSettingsPath,
  runPreviousStatusline,
} from './setup/claude-statusline.mjs';
import { buildMemoryShard, storeMemoryShard, readVerifiedShards } from './memory/store.mjs';
import { rankMemoryShards, renderMemoryRecall } from './memory/recall.mjs';
import { prepareUserPrompt, finalizeUserPrompt } from './hooks/user-prompt.mjs';
import { projectFingerprint } from './lib/fingerprint.mjs';
import { gitContext } from './lib/gitctx.mjs';

function writeStdout(text) {
  return new Promise((resolve, reject) => {
    process.stdout.write(text, (error) => error ? reject(error) : resolve());
  });
}

async function sensorRatelimit(args) {
  const shadow = args.includes('--shadow');
  const readApp = process.env.AH_NO_APPSERVER === '1'
    ? async () => null
    : () => readAppServerRateLimit({});
  const readJsonl = async () => {
    const f = newestSessionFile(join(codexHome(), 'sessions'));
    return f ? readJsonlRateLimit(f) : null;
  };
  const result = await readRateLimit({
    readApp, readJsonl, shadow,
    onMismatch: (app, jsonl) => process.stderr.write(`[shadow] app=${app.usedPercent} jsonl=${jsonl.usedPercent}\n`),
  });
  await writeStdout(JSON.stringify(result) + '\n');
}

function readStdin() {
  return new Promise((resolve) => {
    let value = '';
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', (chunk) => { value += chunk; });
    process.stdin.on('end', () => resolve(value.replace(/^﻿/, '')));
    if (process.stdin.isTTY) resolve('');
  });
}

function argValue(args, name, fallback) {
  const index = args.indexOf(name);
  return index >= 0 && index + 1 < args.length ? args[index + 1] : fallback;
}

// Reads the command's JSON payload in a shell-agnostic way. Source order:
//   --input <file>  (preferred for rich JSON: the caller writes a UTF-8 file
//                    with its native API, so no shell quoting is involved)
//   else stdin.
// A leading UTF-8 BOM (PowerShell pipes add one) is stripped before parsing,
// and --cwd <path> overrides input.cwd so callers never have to embed a
// backslash Windows path inside JSON (argv keeps backslashes literal).
async function readInput(args = []) {
  const file = argValue(args, '--input', null);
  const raw = (file ? readFileSync(file, 'utf8') : await readStdin()).replace(/^﻿/, '');
  let input = {};
  if (raw.trim()) {
    try {
      input = JSON.parse(raw);
    } catch (error) {
      throw new Error(`invalid input JSON from ${file ? `--input ${file}` : 'stdin'}: ${error.message}`);
    }
  }
  if (typeof input !== 'object' || input === null || Array.isArray(input)) input = {};
  const cwd = argValue(args, '--cwd', null);
  if (cwd) input.cwd = cwd;
  return input;
}

function codexSensorReader() {
  const readApp = process.env.AH_NO_APPSERVER === '1' ? async () => null : () => readAppServerRateLimit({});
  const readJsonl = async () => {
    const file = newestSessionFile(join(codexHome(), 'sessions'));
    return file ? readJsonlRateLimit(file) : null;
  };
  return async () => readRateLimit({ readApp, readJsonl });
}

function sensorReader(agent, input, config) {
  if (agent === 'claude-code') {
    return async () => readClaudeRateLimit({
      sessionId: input.session_id,
      freshnessMs: config.sensors?.claude?.freshness_ms ?? 120_000,
    });
  }
  return codexSensorReader();
}

async function sensorClaudeStatusline() {
  const raw = (await readStdin()) || '{}';
  const input = JSON.parse(raw);
  recordClaudeRateLimit(input);
  try { await writeStdout(runPreviousStatusline(raw)); }
  catch (error) { process.stderr.write(`[handoff] previous statusLine failed: ${error.message}\n`); }
}

async function hookStop(args) {
  const agent = argValue(args, '--agent', 'codex');
  const config = loadConfig({ path: configPath() });
  const modeOverride = argValue(args, '--mode', null);
  if (modeOverride) config.triggers.five_hour.mode = modeOverride;
  const input = await readInput(args);
  const result = await handleStop({ input, config, readSensor: sensorReader(agent, input, config), agent });
  process.stderr.write(`[handoff] stop: ${result.action} (${result.reason})\n`);
  if (result.action === 'request-summary') {
    await writeStdout(JSON.stringify({ decision: 'block', reason: result.prompt }) + '\n');
  } else if (result.action === 'ask') {
    await writeStdout(JSON.stringify({
      decision: 'block',
      reason: 'Ask the user once: Capsule을 생성할까요? /handoff create | /handoff skip',
    }) + '\n');
  } else {
    await writeStdout(JSON.stringify({ continue: true }) + '\n');
  }
}

async function deliverSession(input, agent) {
  const result = prepareSessionStart({ input, agent });
  if (!result.injected) return result;
  try {
    await writeStdout(result.context + '\n');
    finalizeSessionStart(result.delivery);
  } catch (error) {
    abortSessionStart(result.delivery);
    throw error;
  }
  return result;
}

async function hookSessionStart(args) {
  const agent = argValue(args, '--agent', 'codex');
  const input = await readInput(args);
  await deliverSession(input, agent);
}

async function handoffStatus(args) {
  const input = await readInput(args);
  await writeStdout(JSON.stringify(statusFor(input.cwd || process.cwd())) + '\n');
}

async function handoffPreview(args) {
  const input = await readInput(args);
  await writeStdout(JSON.stringify(previewFor(input.cwd || process.cwd())) + '\n');
}

async function handoffResume(args) {
  const agent = argValue(args, '--agent', 'codex');
  const input = await readInput(args);
  const result = await deliverSession(input, agent);
  if (!result.injected) process.stderr.write(`[handoff] resume: ${result.reason}\n`);
}

async function handoffCheckpoint(args) {
  const agent = argValue(args, '--agent', 'codex');
  const input = await readInput(args);
  const { capsule, fingerprint } = buildCheckpointCapsule({
    sentinel: input.sentinel || {}, cwd: input.cwd || process.cwd(), agent,
    sessionId: input.session_id, checkpointKey: input.checkpoint_key || randomUUID(),
  });
  publishCapsule(fingerprint, capsule, { status: 'AVAILABLE' });
  await writeStdout(JSON.stringify({ taskId: capsule.task_id, fingerprint }) + '\n');
}

async function handoffCreate(args) {
  const input = await readInput(args);
  await writeStdout(JSON.stringify(createFromApproval({
    cwd: input.cwd || process.cwd(), sentinel: input.sentinel || {},
  })) + '\n');
}

async function handoffSkip(args) {
  const input = await readInput(args);
  await writeStdout(JSON.stringify(skipApproval({ cwd: input.cwd || process.cwd() })) + '\n');
}

async function handoffRecover(args) {
  const input = await readInput(args);
  await writeStdout(JSON.stringify(recoverFor(input.cwd || process.cwd()), null, 2) + '\n');
}

async function hookUserPrompt(args) {
  const input = await readInput(args);
  const config = loadConfig({ path: configPath() });
  if (config.memory?.auto_recall === false) return;
  const result = prepareUserPrompt({
    input, agent: argValue(args, '--agent', 'codex'),
    tokenBudget: config.memory?.auto_recall_token_budget ?? 800,
  });
  if (!result.injected) return;
  await writeStdout(result.context + '\n');
  finalizeUserPrompt(result.delivery);
}

async function memoryRemember(args) {
  const input = await readInput(args);
  const cwd = input.cwd || process.cwd();
  const fingerprint = projectFingerprint(cwd);
  const shard = buildMemoryShard({
    fingerprint, fact: input.fact, evidence: input.evidence || [], tags: input.tags || [],
    paths: input.paths || [], branch: input.branch || gitContext(cwd).branch,
  });
  storeMemoryShard(fingerprint, shard);
  await writeStdout(JSON.stringify({ stored: true, shardId: shard.shard_id, fingerprint }) + '\n');
}

async function memoryRecall(args) {
  const input = await readInput(args);
  const cwd = input.cwd || process.cwd();
  const fingerprint = projectFingerprint(cwd);
  const ranked = rankMemoryShards(readVerifiedShards(fingerprint), {
    prompt: input.prompt || '', paths: input.paths || [], branch: input.branch || gitContext(cwd).branch,
  });
  await writeStdout(renderMemoryRecall(ranked, { tokenBudget: input.token_budget || 800 }));
}

async function setupClaudeStatusline(args) {
  const settingsPath = argValue(args, '--settings', null);
  const result = args.includes('--restore')
    ? restoreClaudeStatusline(settingsPath ? { settingsPath } : {})
    : installClaudeStatusline({
      settingsPath: settingsPath || defaultClaudeSettingsPath(),
      pluginRoot: argValue(args, '--plugin-root', process.env.CLAUDE_PLUGIN_ROOT || process.env.PLUGIN_ROOT),
    });
  await writeStdout(JSON.stringify(result) + '\n');
}

async function configShow() {
  const path = configPath();
  await writeStdout(JSON.stringify({
    path, exists: existsSync(path), keys: knownKeys(), config: loadConfig({ path }),
  }, null, 2) + '\n');
}

async function configGet(args) {
  const input = await readInput(args);
  const config = loadConfig({ path: configPath() });
  await writeStdout(JSON.stringify({ key: input.key, value: getAt(config, input.key) }) + '\n');
}

async function configSet(args) {
  const input = await readInput(args);
  const result = setConfigValue(configPath(), input.key, input.value);
  await writeStdout(JSON.stringify({ ok: true, ...result, path: configPath() }) + '\n');
}

async function configUnset(args) {
  const input = await readInput(args);
  const result = unsetConfigValue(configPath(), input.key);
  await writeStdout(JSON.stringify({ ok: true, ...result, path: configPath() }) + '\n');
}

const [command, ...rest] = process.argv.slice(2);
const commands = {
  'sensor:ratelimit': sensorRatelimit,
  'sensor:claude-statusline': sensorClaudeStatusline,
  'hook:stop': hookStop,
  'hook:session-start': hookSessionStart,
  'hook:user-prompt': hookUserPrompt,
  'handoff:status': handoffStatus,
  'handoff:preview': handoffPreview,
  'handoff:resume': handoffResume,
  'handoff:checkpoint': handoffCheckpoint,
  'handoff:create': handoffCreate,
  'handoff:skip': handoffSkip,
  'handoff:recover': handoffRecover,
  'memory:remember': memoryRemember,
  'memory:recall': memoryRecall,
  'setup:claude-statusline': setupClaudeStatusline,
  'config:show': configShow,
  'config:get': configGet,
  'config:set': configSet,
  'config:unset': configUnset,
};

const run = commands[command];
if (!run) {
  process.stderr.write(`unknown command: ${command ?? '(none)'}\n`);
  process.exit(2);
}
run(rest).catch((error) => {
  process.stderr.write(String(error?.stack || error) + '\n');
  process.exit(1);
});
