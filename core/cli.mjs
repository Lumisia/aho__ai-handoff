import { join } from 'node:path';
import { codexHome, newestSessionFile } from './lib/sessions.mjs';
import { readJsonlRateLimit } from './sensors/codex-jsonl.mjs';
import { readAppServerRateLimit } from './sensors/codex-appserver.mjs';
import { readRateLimit } from './sensors/ratelimit.mjs';

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

const [cmd, ...rest] = process.argv.slice(2);
const commands = { 'sensor:ratelimit': sensorRatelimit };

const run = commands[cmd];
if (!run) {
  process.stderr.write(`unknown command: ${cmd ?? '(none)'}\n`);
  process.exit(2);
}
run(rest).catch((e) => {
  process.stderr.write(String(e?.stack || e) + '\n');
  process.exit(1);
});
