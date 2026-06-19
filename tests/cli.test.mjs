import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const cli = join(here, '..', 'core', 'cli.mjs');

test('sensor:ratelimit prints a json object with a source field', () => {
  // CODEX_HOME을 빈 임시 폴더로 강제 → app-server 비활성 + jsonl 없음 → unknown.
  const out = execFileSync(process.execPath, [cli, 'sensor:ratelimit'], {
    env: { ...process.env, CODEX_HOME: join(here, 'fixtures', '__none__'), AH_NO_APPSERVER: '1' },
    encoding: 'utf8',
  });
  const parsed = JSON.parse(out.trim());
  assert.ok('source' in parsed);
});

test('unknown subcommand exits non-zero', () => {
  assert.throws(() => execFileSync(process.execPath, [cli, 'bogus'], { encoding: 'utf8', stdio: 'pipe' }));
});
