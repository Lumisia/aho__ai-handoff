import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const releaseWorkflow = readFileSync(join(root, '.github/workflows/release.yml'), 'utf8');
const installSh = readFileSync(join(root, 'scripts/install.sh'), 'utf8');
const installPs1 = readFileSync(join(root, 'scripts/install.ps1'), 'utf8');
const required = [
  '.claude-plugin/plugin.json', '.codex-plugin/plugin.json',
  '.claude-plugin/marketplace.json', '.agents/plugins/marketplace.json',
  'scripts/install.sh',
  'skills/handoff/SKILL.md',
  'skills/handoff-checkpoint/SKILL.md',
  'skills/handoff-doctor/SKILL.md',
  'skills/handoff-config/SKILL.md',
  'schemas/capsule.schema.json', 'schemas/memory-shard.schema.json',
];
for (const relative of required) {
  if (!existsSync(join(root, relative))) throw new Error(`missing package file: ${relative}`);
}
const claude = JSON.parse(readFileSync(join(root, '.claude-plugin/plugin.json'), 'utf8'));
const codex = JSON.parse(readFileSync(join(root, '.codex-plugin/plugin.json'), 'utf8'));
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
if (claude.name !== codex.name || claude.version !== codex.version || pkg.version !== codex.version) {
  throw new Error('manifest mismatch');
}
if (claude.experimental?.monitors || codex.hooks) {
  throw new Error('source plugin must not expose legacy v1 monitors or hook templates');
}
for (const skill of ['handoff', 'handoff-config', 'handoff-doctor', 'handoff-checkpoint']) {
  const text = readFileSync(join(root, 'skills', skill, 'SKILL.md'), 'utf8');
  if (!text.startsWith('---') || !text.includes('name:') || !text.includes('description:')) {
    throw new Error(`invalid skill frontmatter: ${skill}`);
  }
}
const claudeMarket = JSON.parse(readFileSync(join(root, '.claude-plugin/marketplace.json'), 'utf8'));
const codexMarket = JSON.parse(readFileSync(join(root, '.agents/plugins/marketplace.json'), 'utf8'));
for (const [label, market] of [['claude', claudeMarket], ['codex', codexMarket]]) {
  if (market.name !== 'claude-codex-auto-handoff') throw new Error(`${label} marketplace name mismatch`);
  if (!(market.plugins || []).some((entry) => entry.name === claude.name)) {
    throw new Error(`${label} marketplace does not list plugin ${claude.name}`);
  }
}
for (const target of [
  'linux-x86_64',
  'linux-aarch64',
  'darwin-aarch64',
  'darwin-x86_64',
  'windows-x86_64',
  'windows-aarch64',
]) {
  if (!releaseWorkflow.includes(`target_name: ${target}`)) {
    throw new Error(`release workflow missing target: ${target}`);
  }
}
if (!releaseWorkflow.includes('.sha256') || !releaseWorkflow.includes('Get-FileHash')) {
  throw new Error('release workflow must publish sha256 checksum files');
}
if (!installSh.includes('.sha256') || !installSh.includes('sha256sum') || !installSh.includes('shasum -a 256')) {
  throw new Error('install.sh must verify sha256 checksums');
}
if (!installPs1.includes('.sha256') || !installPs1.includes('Get-FileHash')) {
  throw new Error('install.ps1 must verify sha256 checksums');
}
process.stdout.write(`package valid: ${claude.name}@${claude.version} (marketplace: ${claudeMarket.name})\n`);
