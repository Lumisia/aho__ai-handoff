import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const required = [
  '.claude-plugin/plugin.json', '.codex-plugin/plugin.json', 'hooks/hooks.json',
  'scripts/run-hook.mjs', 'core/cli.mjs', 'schemas/capsule.schema.json',
  'schemas/memory-shard.schema.json',
];
for (const relative of required) {
  if (!existsSync(join(root, relative))) throw new Error(`missing package file: ${relative}`);
}
const claude = JSON.parse(readFileSync(join(root, '.claude-plugin/plugin.json'), 'utf8'));
const codex = JSON.parse(readFileSync(join(root, '.codex-plugin/plugin.json'), 'utf8'));
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
const hooks = JSON.parse(readFileSync(join(root, 'hooks/hooks.json'), 'utf8'));
if (claude.name !== codex.name || claude.version !== codex.version || pkg.version !== codex.version) {
  throw new Error('manifest mismatch');
}
for (const event of ['SessionStart', 'Stop', 'UserPromptSubmit']) {
  if (!Array.isArray(hooks.hooks?.[event])) throw new Error(`missing hook event: ${event}`);
}
process.stdout.write(`package valid: ${claude.name}@${claude.version}\n`);
