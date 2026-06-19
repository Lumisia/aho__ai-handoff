import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, resolve } from 'node:path';

const EVENT_COMMANDS = {
  'session-start': 'hook:session-start',
  stop: 'hook:stop',
  'user-prompt': 'hook:user-prompt',
};

export function resolveHookInvocation(event, env = process.env) {
  const command = EVENT_COMMANDS[event];
  if (!command) throw new Error(`unknown hook event: ${event}`);
  const pluginRoot = env.PLUGIN_ROOT || env.CLAUDE_PLUGIN_ROOT;
  if (!pluginRoot) throw new Error('plugin root environment variable is missing');
  const agent = env.PLUGIN_ROOT ? 'codex' : 'claude-code';
  return { pluginRoot, agent, command };
}

export function runHook(event, { env = process.env, input } = {}) {
  const invocation = resolveHookInvocation(event, env);
  const raw = input ?? readFileSync(0);
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
