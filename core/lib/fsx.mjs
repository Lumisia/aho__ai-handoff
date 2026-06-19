import {
  openSync, writeSync, fsyncSync, closeSync, renameSync, mkdirSync,
  existsSync, readFileSync, unlinkSync,
} from 'node:fs';
import { dirname } from 'node:path';

export function writeFileAtomic(path, data) {
  mkdirSync(dirname(path), { recursive: true });
  const tmp = `${path}.tmp-${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  const fd = openSync(tmp, 'w');
  try {
    writeSync(fd, typeof data === 'string' ? data : Buffer.from(data));
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  renameSync(tmp, path);
}

// Single-owner lease lock backed by atomic exclusive file creation.
export function acquireLock(lockPath, { leaseMs = 30000, now = Date.now() } = {}) {
  const token = `${process.pid}-${Math.random().toString(36).slice(2)}`;
  const content = JSON.stringify({ token, expiresAt: now + leaseMs });
  mkdirSync(dirname(lockPath), { recursive: true });
  for (let attempt = 0; attempt < 2; attempt++) {
    let fd;
    try {
      fd = openSync(lockPath, 'wx');
      writeSync(fd, content);
      fsyncSync(fd);
      closeSync(fd);
      return { token, lockPath };
    } catch (error) {
      if (fd !== undefined) { try { closeSync(fd); } catch {} }
      if (error?.code !== 'EEXIST') throw error;
      let expiresAt = 0;
      try { expiresAt = JSON.parse(readFileSync(lockPath, 'utf8')).expiresAt || 0; } catch {}
      if (expiresAt > now) return null;
      try { unlinkSync(lockPath); } catch { return null; }
    }
  }
  return null;
}

export function ownsLock(lock) {
  if (!lock) return false;
  try { return JSON.parse(readFileSync(lock.lockPath, 'utf8')).token === lock.token; }
  catch { return false; }
}

export function releaseLock(lock) {
  if (!lock) return;
  try {
    const cur = JSON.parse(readFileSync(lock.lockPath, 'utf8'));
    if (cur.token === lock.token) unlinkSync(lock.lockPath);
  } catch {}
}
