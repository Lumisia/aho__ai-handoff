import { dirname, join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
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
  statusFor, previewFor, createFromApproval, skipApproval, doctorFor, recentCapsules,
} from './hooks/handoff.mjs';
import { buildCheckpointCapsule } from './capsule/checkpoint.mjs';
import { publishCapsule } from './capsule/store.mjs';
import {
  clearCapsules, clearProjectState, clearSummary, configuredOlderThanMs, parseOlderThan, normalizeClearScope,
} from './capsule/clear.mjs';
import { consumeOnPrompt } from './capsule/inject-track.mjs';
import { findNewerPending, recordNotified, renderPendingNotice } from './capsule/pending-notice.mjs';
import {
  installClaudeStatusline, restoreClaudeStatusline, defaultClaudeSettingsPath,
  runPreviousStatusline, DEFAULT_CLAUDE_STATUSLINE_REFRESH_INTERVAL,
} from './setup/claude-statusline.mjs';
import { buildMemoryShard, storeMemoryShard, readVerifiedShards } from './memory/store.mjs';
import { rankMemoryShards, renderMemoryRecall } from './memory/recall.mjs';
import { prepareUserPrompt, finalizeUserPrompt } from './hooks/user-prompt.mjs';
import { prepareTurnHandoff } from './hooks/turn-handoff.mjs';
import { projectFingerprint } from './lib/fingerprint.mjs';
import { t, askInstruction } from './lib/i18n.mjs';
import { additionalContextOutput, continueOutput, stopContinuationOutput } from './lib/hook-output.mjs';
import { readHistory } from './capsule/history.mjs';
import { gitContext } from './lib/gitctx.mjs';
import { statuslineSegment } from './lib/statusline-segment.mjs';

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

function runtimePluginRoot() {
  return dirname(dirname(fileURLToPath(import.meta.url)));
}

function configuredPluginRoot() {
  return process.env.CLAUDE_PLUGIN_ROOT || process.env.PLUGIN_ROOT || runtimePluginRoot();
}

function optionValue(args, name, fallback = null) {
  const eq = args.find((a) => a.startsWith(`${name}=`));
  if (eq) return eq.slice(name.length + 1);
  return argValue(args, name, fallback);
}

function positionalArgs(args) {
  const valueOptions = new Set(['--cwd', '--input', '--agent', '--limit', '--mode', '--older-than']);
  const out = [];
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === '-c' || a === '--confirm' || a === '--yes') continue;
    if (a.startsWith('--')) {
      if (!a.includes('=') && valueOptions.has(a)) i++;
      continue;
    }
    if (a.startsWith('-')) continue;
    out.push(a);
  }
  return out;
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
      freshnessMs: config.sensors?.claude?.freshness_ms ?? 10_000,
    });
  }
  return codexSensorReader();
}

async function sensorClaudeStatusline() {
  const raw = (await readStdin()) || '{}';
  const input = JSON.parse(raw);
  recordClaudeRateLimit(input);
  const cfg = loadConfig({ path: configPath() });
  const seg = statuslineSegment({
    usedPercent: input?.rate_limits?.five_hour?.used_percentage,
    cwd: input.cwd || input.workspace?.current_dir,
    show: cfg.statusline?.show_handoff !== false,
  });
  let prev = '';
  try { prev = runPreviousStatusline(raw); }
  catch (error) { process.stderr.write(`[handoff] previous statusLine failed: ${error.message}\n`); }
  await writeStdout(seg ? (prev ? `${seg} | ${prev}` : seg + '\n') : prev);
}

async function hookStop(args) {
  const agent = argValue(args, '--agent', 'codex');
  const config = loadConfig({ path: configPath() });
  const locale = config.locale || 'en';
  const modeOverride = argValue(args, '--mode', null);
  if (modeOverride) config.triggers.five_hour.mode = modeOverride;
  const input = await readInput(args);
  const result = await handleStop({ input, config, readSensor: sensorReader(agent, input, config), agent });
  process.stderr.write(`[handoff] stop: ${result.action} (${result.reason})\n`);
  if (result.action === 'request-summary') {
    const allowCodex = agent !== 'codex' || config.codex?.stop_continuation_auto_summary === true;
    await writeStdout(JSON.stringify(stopContinuationOutput(agent, result.prompt, { allowCodex })) + '\n');
  } else if (result.action === 'ask') {
    const allowCodex = agent !== 'codex' || config.codex?.stop_continuation_ask === true;
    await writeStdout(JSON.stringify(stopContinuationOutput(agent, askInstruction(agent, locale), { allowCodex })) + '\n');
  } else {
    await writeStdout(JSON.stringify(continueOutput()) + '\n');
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

function autoInstallClaudeStatusline(agent) {
  if (agent !== 'claude-code') return;

  try {
    const result = installClaudeStatusline({
      settingsPath: defaultClaudeSettingsPath(),
      pluginRoot: configuredPluginRoot(),
      refreshInterval: DEFAULT_CLAUDE_STATUSLINE_REFRESH_INTERVAL,
      stableShim: true,
      auto: true,
    });

    if (process.env.AI_HANDOFF_DEBUG === '1' && result.reason) {
      process.stderr.write(`[handoff] Claude statusline auto-setup skipped: ${result.reason}\n`);
    }
  } catch (error) {
    process.stderr.write(`[handoff] Claude statusline auto-setup failed: ${error.message}\n`);
  }
}

async function hookSessionStart(args) {
  const agent = argValue(args, '--agent', 'codex');
  const input = await readInput(args);
  const config = loadConfig({ path: configPath() });
  autoInstallClaudeStatusline(agent);
  if (config.clear?.auto?.enabled === true) {
    try {
      clearCapsules({
        cwd: input.cwd || process.cwd(),
        scope: 'used',
        olderThanMs: configuredOlderThanMs(config),
      });
    } catch (error) {
      process.stderr.write(`[handoff] auto-clear failed: ${error.message}\n`);
    }
  }
  if (config.handoff?.session_start_auto_fetch === true) {
    await deliverSession(input, agent);
  }
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
  // An explicit /handoff resume injects and consumes within THIS single process,
  // so there is no cross-session window. If the caller supplied no session id,
  // synthesize a unique one for this invocation rather than sharing the global
  // 'unknown' inject key (which a different id-less session could otherwise
  // piggyback on, consuming a capsule it never saw).
  if (!input.session_id) input.session_id = `resume-${randomUUID()}`;
  const result = await deliverSession(input, agent);
  if (!result.injected) { process.stderr.write(`[handoff] resume: ${result.reason}\n`); return; }
  // A deliberate user action is proof of life — consume now rather than waiting
  // for the next prompt.
  const consumed = consumeOnPrompt({ input, agent });
  process.stderr.write(`[handoff] resume: injected, consumed=${consumed.consumed}\n`);
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

async function handoffDoctor(args) {
  const input = await readInput(args);
  const cwd = input.cwd || process.cwd();
  let fixStatusline = null;
  if (args.includes('--fix-statusline')) {
    const pluginRoot = argValue(args, '--plugin-root', configuredPluginRoot());
    fixStatusline = installClaudeStatusline({
      settingsPath: defaultClaudeSettingsPath(),
      pluginRoot,
      refreshInterval: DEFAULT_CLAUDE_STATUSLINE_REFRESH_INTERVAL,
      stableShim: true,
      auto: false,
      force: true,
    });
  }
  const report = doctorFor(cwd);
  if (fixStatusline) report.fixStatusline = fixStatusline;
  await writeStdout(JSON.stringify(report, null, 2) + '\n');
}

async function handoffHistory(args) {
  const input = await readInput(args);
  const limit = Number(argValue(args, '--limit', '20')) || 20;
  const fp = projectFingerprint(input.cwd || process.cwd());
  await writeStdout(JSON.stringify(readHistory(fp, { limit }), null, 2) + '\n');
}

async function handoffRecent(args) {
  const input = await readInput(args);
  const limit = Number(argValue(args, '--limit', '10')) || 10;
  const currentFingerprint = projectFingerprint(input.cwd || process.cwd());
  await writeStdout(JSON.stringify(recentCapsules({ limit, currentFingerprint }), null, 2) + '\n');
}

async function handoffClear(args) {
  const input = await readInput(args);
  const config = loadConfig({ path: configPath() });
  const cwd = input.cwd || process.cwd();
  const olderThanRaw = optionValue(args, '--older-than', input.older_than ?? input.olderThan ?? null);
  const scope = normalizeClearScope(positionalArgs(args)[0] || input.scope || (olderThanRaw != null ? 'used' : ''));
  const confirmed = args.includes('-c') || args.includes('--confirm') || args.includes('--yes') || input.confirmed === true;
  let result;
  if (!scope) {
    result = clearSummary({ cwd, config });
  } else if (scope === 'this_project') {
    result = clearProjectState({ cwd, confirmed });
  } else {
    const olderThanMs = olderThanRaw != null
      ? parseOlderThan(olderThanRaw)
      : (scope === 'pending' ? null : configuredOlderThanMs(config));
    result = clearCapsules({ cwd, scope, olderThanMs });
  }
  await writeStdout(JSON.stringify(result, null, 2) + '\n');
}

async function hookUserPrompt(args) {
  const agent = argValue(args, '--agent', 'codex');
  const input = await readInput(args);
  const config = loadConfig({ path: configPath() });
  // If SessionStart auto-fetch is enabled, the first prompt proves the session
  // is live and can consume the read-only injection. Best-effort: never break
  // the prompt path.
  if (config.handoff?.session_start_auto_fetch === true) {
    try { consumeOnPrompt({ input, agent }); }
    catch (error) { process.stderr.write(`[handoff] consume-on-prompt failed: ${error.message}\n`); }
  }
  const parts = [];

  try {
    const handoff = await prepareTurnHandoff({
      input,
      config,
      readSensor: sensorReader(agent, input, config),
      agent,
    });
    if (handoff.injected) parts.push(handoff.context);
  } catch (error) {
    process.stderr.write(`[handoff] turn-handoff failed: ${error.message}\n`);
  }

  // A peer checkpoint created after this session started will not reach a
  // running session on its own. Surface it as a one-time nudge with key info so
  // the model can pull it with /handoff. Best-effort.
  if (config.handoff?.notify_newer_pending !== false) {
    try {
      const notice = findNewerPending({ input, agent });
      if (notice.notify) {
        parts.push(renderPendingNotice(notice.capsule, config.locale || 'en'));
        recordNotified({ fingerprint: notice.fingerprint, sessionId: notice.sessionId, taskId: notice.taskId });
      }
    } catch (error) { process.stderr.write(`[handoff] newer-pending notice failed: ${error.message}\n`); }
  }

  if (config.memory?.auto_recall !== false) {
    const result = prepareUserPrompt({
      input, agent,
      tokenBudget: config.memory?.auto_recall_token_budget ?? 800,
    });
    if (result.injected) {
      parts.push(result.context);
      finalizeUserPrompt(result.delivery);
    }
  }

  if (parts.length) await writeStdout(parts.join('\n\n') + '\n');
}

async function hookPostToolUse(args) {
  const agent = argValue(args, '--agent', 'codex');
  const input = await readInput(args);
  const config = loadConfig({ path: configPath() });

  try {
    const handoff = await prepareTurnHandoff({
      input,
      config,
      readSensor: sensorReader(agent, input, config),
      agent,
    });
    const output = handoff.injected
      ? additionalContextOutput('PostToolUse', handoff.context)
      : continueOutput();
    await writeStdout(JSON.stringify(output) + '\n');
  } catch (error) {
    process.stderr.write(`[handoff] post-tool-use handoff failed: ${error.message}\n`);
    await writeStdout(JSON.stringify(continueOutput()) + '\n');
  }
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
  const refreshRaw = Number.parseInt(argValue(args, '--refresh-interval', String(DEFAULT_CLAUDE_STATUSLINE_REFRESH_INTERVAL)), 10);
  const refreshInterval = Number.isFinite(refreshRaw) ? refreshRaw : DEFAULT_CLAUDE_STATUSLINE_REFRESH_INTERVAL;
  const result = args.includes('--restore')
    ? restoreClaudeStatusline(settingsPath ? { settingsPath } : {})
    : installClaudeStatusline({
      settingsPath: settingsPath || defaultClaudeSettingsPath(),
      pluginRoot: argValue(args, '--plugin-root', configuredPluginRoot()),
      refreshInterval,
      stableShim: !args.includes('--direct'),
      auto: args.includes('--auto'),
      force: args.includes('--force'),
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
  'hook:post-tool-use': hookPostToolUse,
  'handoff:status': handoffStatus,
  'handoff:preview': handoffPreview,
  'handoff:resume': handoffResume,
  'handoff:checkpoint': handoffCheckpoint,
  'handoff:create': handoffCreate,
  'handoff:skip': handoffSkip,
  'handoff:doctor': handoffDoctor,
  'handoff:history': handoffHistory,
  'handoff:recent': handoffRecent,
  'handoff:clear': handoffClear,
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
