import { test } from 'node:test';
import assert from 'node:assert/strict';
import { appServerSpawnSpec } from '../core/sensors/codex-appserver.mjs';

test('POSIX app-server spawn avoids a shell', () => {
  assert.deepEqual(appServerSpawnSpec('codex', { platform: 'linux' }), {
    file: 'codex', args: ['app-server', '--stdio'], options: { shell: false, windowsHide: true },
  });
});

test('Windows app-server uses explicit cmd invocation for npm command shims', () => {
  const spec = appServerSpawnSpec('codex', { platform: 'win32', comspec: 'cmd.exe' });
  assert.equal(spec.file, 'cmd.exe');
  assert.deepEqual(spec.args.slice(0, 3), ['/d', '/s', '/c']);
  assert.match(spec.args[3], /codex.*app-server.*--stdio/);
  assert.equal(spec.options.shell, false);
  assert.equal(spec.options.windowsVerbatimArguments, true);
});

test('rejects a command containing shell metacharacters', () => {
  assert.throws(() => appServerSpawnSpec('codex & whoami', { platform: 'win32' }), /unsafe command/);
});
