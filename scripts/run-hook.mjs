import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { acquireLock } from '../core/lib/fsx.mjs';

const EVENT_COMMANDS = {
  'session-start': 'hook:session-start',
  stop: 'hook:stop',
  'user-prompt': 'hook:user-prompt',
};

// How long two firings of the same event count as the same occurrence. Duplicate
// firings from double hook registration arrive near-simultaneously; legitimate
// repeats of an event (a later prompt, a later stop) are both separated by far
// more than this AND carry a different payload, so they are never suppressed.
const DEDUPE_WINDOW_MS = 5000;

export function resolveHookInvocation(event, env = process.env) {
  const command = EVENT_COMMANDS[event];
  if (!command) throw new Error(`unknown hook event: ${event}`);
  const pluginRoot = env.PLUGIN_ROOT || env.CLAUDE_PLUGIN_ROOT;
  if (!pluginRoot) throw new Error('plugin root environment variable is missing');
  const agent = env.PLUGIN_ROOT ? 'codex' : 'claude-code';
  return { pluginRoot, agent, command };
}

// Claim a single event occurrence so duplicate firings run the hook once. Codex
// registers both the auto-discovered hooks.json and the manifest hooks-codex.json,
// so each event fires twice with identical stdin; without this guard the Stop
// capsule build, SessionStart claim, and memory injection would all run twice and
// race on the same lock. The claim is an unreleased lease keyed on the event,
// agent, and exact payload: the first firing takes the lease, an identical sibling
// within the window is rejected, and the lease expires so the next real event runs.
// Returns true if this process should run the hook, false if a sibling already did.
export function claimHookEvent({ event, agent, raw, dir = join(tmpdir(), 'ai-handoff-hookguard'), now = Date.now(), windowMs = DEDUPE_WINDOW_MS }) {
  const key = createHash('sha256').update(`${event}\0${agent}\0`).update(raw ?? '').digest('hex').slice(0, 32);
  return acquireLock(join(dir, `${key}.lock`), { leaseMs: windowMs, now }) !== null;
}

export function runHook(event, { env = process.env, input } = {}) {
  const invocation = resolveHookInvocation(event, env);
  const raw = input ?? readFileSync(0);
  if (!claimHookEvent({ event, agent: invocation.agent, raw })) return 0;
  const child = spawnSync(process.execPath, [
    join(invocation.pluginRoot, 'core', 'cli.mjs'), invocation.command, '--agent', invocation.agent,
  ], { input: raw, encoding: 'utf8', env });
  if (child.stdout) process.stdout.write(child.stdout);
  if (child.stderr) process.stderr.write(child.stderr);
  if (child.error) throw child.error;
  return child.status ?? 1;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  try { process.exitCode = runHook(process.argv[2]); }
  catch (error) {
    process.stderr.write(`${error?.stack || error}\n`);
    process.exitCode = 1;
  }
}
