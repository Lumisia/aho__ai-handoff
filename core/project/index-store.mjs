import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { projectDir } from '../lib/paths.mjs';
import { writeFileAtomic } from '../lib/fsx.mjs';
import { buildManifest, diffManifest } from './manifest.mjs';
import { renderIndex } from './index-md.mjs';

function readJson(path) {
  try { return JSON.parse(readFileSync(path, 'utf8')); } catch { return null; }
}

function flatFiles(manifest) {
  return Object.fromEntries(Object.entries(manifest?.files || {}).map(([path, value]) => [
    path, typeof value === 'string' ? value : value.sha256,
  ]));
}

export function refreshProjectIndex(fingerprint, taskId, { now = Date.now() } = {}) {
  const root = projectDir(fingerprint);
  const manifestPath = join(root, 'manifest.json');
  const indexPath = join(root, 'INDEX.md');
  const oldManifest = readJson(manifestPath);
  const current = buildManifest(join(root, 'project'), { now });
  const changed = diffManifest({ files: flatFiles(oldManifest) }, current);
  const dirtyPaths = new Set(changed.filter((item) => item.status !== 'DELETED').map((item) => item.path));
  const manifest = {
    version: now,
    dirty: changed.length > 0,
    files: Object.fromEntries(Object.entries(current.files).map(([path, sha256]) => [
      path, { sha256, dirty: dirtyPaths.has(path) },
    ])),
    changed,
  };
  writeFileAtomic(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  writeFileAtomic(indexPath, renderIndex({ project: fingerprint, changed, taskId }));
  return { manifest, changed, manifestPath, indexPath };
}

export function readThinProjectIndex(fingerprint, { maxChars = 4000 } = {}) {
  const path = join(projectDir(fingerprint), 'INDEX.md');
  if (!existsSync(path)) return '';
  return readFileSync(path, 'utf8').slice(0, maxChars);
}
