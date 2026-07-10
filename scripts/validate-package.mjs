import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const releaseWorkflow = readFileSync(join(root, '.github/workflows/release.yml'), 'utf8');
const installSh = readFileSync(join(root, 'scripts/install.sh'), 'utf8');
const installPs1 = readFileSync(join(root, 'scripts/install.ps1'), 'utf8');
const nsisHooks = readFileSync(join(root, 'apps/desktop/src-tauri/nsis-hooks.nsh'), 'utf8');
const tauriConf = JSON.parse(
  readFileSync(join(root, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'),
);
const tauriSidecarsConf = JSON.parse(
  readFileSync(join(root, 'apps/desktop/src-tauri/tauri.sidecars.conf.json'), 'utf8'),
);
const required = [
  '.claude-plugin/plugin.json', '.codex-plugin/plugin.json',
  '.claude-plugin/marketplace.json', '.agents/plugins/marketplace.json',
  'scripts/install.sh',
  'skills/handoff/SKILL.md',
  'skills/handoff-checkpoint/SKILL.md',
  'skills/handoff-doctor/SKILL.md',
  'skills/handoff-config/SKILL.md',
  'schemas/capsule.schema.json',
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
if (!releaseWorkflow.includes('cargo build --release -p ai-handoff-cli --bins')) {
  throw new Error('release workflow must build both CLI binaries');
}
for (const binary of ['ai-handoff-host', 'ai-handoff-host.exe']) {
  if (!releaseWorkflow.includes(binary)) {
    throw new Error(`release workflow must package ${binary}`);
  }
}
for (const target of ['windows-x86_64', 'windows-aarch64']) {
  if (!releaseWorkflow.includes(`ai-handoff-gui-${target}-setup.exe`)) {
    throw new Error(`release workflow missing Windows GUI artifact: ${target}`);
  }
}
if (!installSh.includes('.sha256') || !installSh.includes('sha256sum') || !installSh.includes('shasum -a 256')) {
  throw new Error('install.sh must verify sha256 checksums');
}
if (!installPs1.includes('.sha256') || !installPs1.includes('Get-FileHash')) {
  throw new Error('install.ps1 must verify sha256 checksums');
}
if (!installSh.includes('ai-handoff-host') || !installPs1.includes('ai-handoff-host.exe')) {
  throw new Error('CLI installers must install the native host beside the CLI');
}
if (installPs1.includes('-WithGui is not available')) {
  throw new Error('install.ps1 -WithGui must install the Windows GUI artifact');
}
if (!installPs1.includes('Install-Gui')) {
  throw new Error('install.ps1 must expose an Install-Gui helper');
}
if (tauriConf.bundle?.active !== true) {
  throw new Error('Tauri desktop bundle must be active for release builds');
}
if (!tauriConf.bundle?.icon?.includes('icons/icon.ico')) {
  throw new Error('Tauri desktop bundle must use icons/icon.ico');
}
for (const sidecar of ['binaries/ai-handoff', 'binaries/ai-handoff-host']) {
  if (!tauriSidecarsConf.bundle?.externalBin?.includes(sidecar)) {
    throw new Error(`Tauri desktop bundle must include sidecar: ${sidecar}`);
  }
}
if (!releaseWorkflow.includes('--config src-tauri/tauri.sidecars.conf.json')) {
  throw new Error('desktop release must enable the sidecar-only Tauri config overlay');
}
if (!releaseWorkflow.includes('binaries/ai-handoff-host-${{ matrix.rust_target }}.exe')) {
  throw new Error('desktop release must stage the native host sidecar');
}
if (!nsisHooks.includes('ai-handoff-host.exe') || !nsisHooks.includes(' install --yes')) {
  throw new Error('NSIS installer must provision both managed binaries and register the integration');
}
if (!nsisHooks.includes(' uninstall --keep-store --yes')) {
  throw new Error('NSIS uninstaller must remove host registration before deleting files');
}
if (!nsisHooks.includes('AI Handoff CLI integration failed to uninstall')) {
  throw new Error('NSIS uninstaller must stop when managed integration cleanup fails');
}
process.stdout.write(`package valid: ${claude.name}@${claude.version} (marketplace: ${claudeMarket.name})\n`);
