import {
  openSync, writeSync, fsyncSync, closeSync, renameSync, mkdirSync,
  existsSync, readFileSync, unlinkSync, statSync,
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
      const code = error?.code;
      if (code !== 'EEXIST') {
        // On Windows a contended exclusive create can transiently fail with
        // EPERM/EACCES/EBUSY (the lockfile is delete-pending from another
        // process's release, or sharing-locked). Treat it as "held right now"
        // so withLock backs off and retries instead of crashing the hook.
        if (code === 'EPERM' || code === 'EACCES' || code === 'EBUSY') return null;
        throw error;
      }
      let info = null;
      try { info = JSON.parse(readFileSync(lockPath, 'utf8')); } catch {}
      if (info && info.expiresAt > now) return null; // a live lease holds it
      if (!info) {
        // Lockfile exists but is empty/unparseable: either a holder that has
        // created it but not yet written (a sub-millisecond window) or one that
        // crashed mid-write. Don't steal a fresh one — back off and retry; only
        // reclaim if it is older than a full lease.
        let ageMs = Infinity;
        try { ageMs = now - statSync(lockPath).mtimeMs; } catch {}
        if (ageMs < leaseMs) return null;
      }
      try { unlinkSync(lockPath); } catch { return null; }
    }
  }
  return null;
}

export function sleepSync(ms) {
  try { Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms); }
  catch { const end = Date.now() + ms; while (Date.now() < end) { /* busy-wait fallback */ } }
}

// Run `fn` while holding a short lease lock, retrying with backoff so concurrent
// read-modify-write appends serialize instead of clobbering each other. After the
// retry budget is exhausted it runs `fn` anyway: a best-effort write beats throwing
// from a fire-and-forget hook path (e.g. publishCapsule -> appendHistory).
export function withLock(lockPath, fn, { leaseMs = 3000, tries = 600, waitMs = 15 } = {}) {
  for (let i = 0; i < tries; i++) {
    const lock = acquireLock(lockPath, { leaseMs, now: Date.now() });
    if (lock) {
      try { return fn(); }
      finally { releaseLock(lock); }
    }
    sleepSync(waitMs);
  }
  return fn();
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
