import { join } from 'node:path';
import { codexHome, newestSessionFile } from './lib/sessions.mjs';
import { readJsonlRateLimit } from './sensors/codex-jsonl.mjs';
import { readAppServerRateLimit } from './sensors/codex-appserver.mjs';
import { readRateLimit } from './sensors/ratelimit.mjs';
import { handleStop } from './hooks/stop.mjs';
import { handleSessionStart } from './hooks/session-start.mjs';
import { loadConfig } from './lib/config.mjs';
import { configPath } from './lib/paths.mjs';
import { statusFor, previewFor } from './hooks/handoff.mjs';
import { buildCheckpointCapsule } from './capsule/checkpoint.mjs';
import { publishCapsule } from './capsule/store.mjs';

async function sensorRatelimit(args) {
  const shadow = args.includes('--shadow');
  const readApp = process.env.AH_NO_APPSERVER === '1'
    ? async () => null
    : () => readAppServerRateLimit({});
  const readJsonl = async () => {
    const f = newestSessionFile(join(codexHome(), 'sessions'));
    return f ? readJsonlRateLimit(f) : null;
  };
  const r = await readRateLimit({
    readApp,
    readJsonl,
    shadow,
    onMismatch: (a, j) =>
      process.stderr.write(`[shadow] app=${a.usedPercent} jsonl=${j.usedPercent}\n`),
  });
  process.stdout.write(JSON.stringify(r) + '\n');
}

function readStdin() {
  return new Promise((resolve) => {
    let s = '';
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', (d) => { s += d; });
    process.stdin.on('end', () => resolve(s));
    if (process.stdin.isTTY) resolve('');
  });
}

function argValue(args, name, fallback) {
  const i = args.indexOf(name);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : fallback;
}

function sensorReader() {
  const readApp = process.env.AH_NO_APPSERVER === '1' ? async () => null : () => readAppServerRateLimit({});
  const readJsonl = async () => {
    const f = newestSessionFile(join(codexHome(), 'sessions'));
    return f ? readJsonlRateLimit(f) : null;
  };
  return async () => readRateLimit({ readApp, readJsonl });
}

async function hookStop(args) {
  const agent = argValue(args, '--agent', 'codex');
  const config = loadConfig({ path: configPath() });
  const modeOverride = argValue(args, '--mode', null);
  if (modeOverride) config.triggers.five_hour.mode = modeOverride;
  const input = JSON.parse((await readStdin()) || '{}');
  const r = await handleStop({ input, config, readSensor: sensorReader(), agent });
  process.stderr.write(`[handoff] stop: ${r.action} (${r.reason})\n`);
}

async function hookSessionStart() {
  const input = JSON.parse((await readStdin()) || '{}');
  const r = handleSessionStart({ input });
  if (r.injected) process.stdout.write(r.context + '\n');
}

async function handoffStatus() {
  const input = JSON.parse((await readStdin()) || '{}');
  process.stdout.write(JSON.stringify(statusFor(input.cwd || process.cwd())) + '\n');
}

async function handoffPreview() {
  const input = JSON.parse((await readStdin()) || '{}');
  process.stdout.write(JSON.stringify(previewFor(input.cwd || process.cwd())) + '\n');
}

async function handoffResume() {
  const input = JSON.parse((await readStdin()) || '{}');
  const r = handleSessionStart({ input });
  if (r.injected) process.stdout.write(r.context + '\n');
  else process.stderr.write(`[handoff] resume: ${r.reason}\n`);
}

async function handoffCheckpoint(args) {
  const agent = argValue(args, '--agent', 'codex');
  const input = JSON.parse((await readStdin()) || '{}');
  const { capsule, fingerprint } = buildCheckpointCapsule({
    sentinel: input.sentinel || {},
    cwd: input.cwd || process.cwd(),
    agent,
    sessionId: input.session_id,
  });
  publishCapsule(fingerprint, capsule, { status: 'AVAILABLE' });
  process.stdout.write(JSON.stringify({ taskId: capsule.task_id, fingerprint }) + '\n');
}

const [cmd, ...rest] = process.argv.slice(2);
const commands = {
  'sensor:ratelimit': sensorRatelimit,
  'hook:stop': hookStop,
  'hook:session-start': hookSessionStart,
  'handoff:status': handoffStatus,
  'handoff:preview': handoffPreview,
  'handoff:resume': handoffResume,
  'handoff:checkpoint': handoffCheckpoint,
};

const run = commands[cmd];
if (!run) {
  process.stderr.write(`unknown command: ${cmd ?? '(none)'}\n`);
  process.exit(2);
}
run(rest).catch((e) => {
  process.stderr.write(String(e?.stack || e) + '\n');
  process.exit(1);
});
