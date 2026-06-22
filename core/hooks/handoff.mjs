import { projectFingerprint, projectFingerprintInfo } from '../lib/fingerprint.mjs';
import { appendHistory, readHistory } from '../capsule/history.mjs';
import { readdirSync, readFileSync, existsSync, realpathSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { findPendingCapsule, verifyStoredCapsule } from '../capsule/store.mjs';
import { publishCapsule } from '../capsule/store.mjs';
import { findApproval, resolveApproval, restoreApprovalForRetry } from '../capsule/approval.mjs';
import { buildCheckpointCapsule } from '../capsule/checkpoint.mjs';
import { dataRoot, handoffDir } from '../lib/paths.mjs';
import { stateReport } from '../lib/state-report.mjs';

export function statusFor(cwd) {
  const fp = projectFingerprint(cwd);
  const p = findPendingCapsule(fp);
  const approval = findApproval(fp);
  return {
    fingerprint: fp,
    pending: !!(p && p.capsule),
    taskId: p && p.taskId,
    state: p && p.state && p.state.status,
    awaitingUser: !!approval,
    approvalKey: approval && approval.key,
  };
}

export function createFromApproval({ cwd, sentinel = {}, now = Date.now() }) {
  const fp = projectFingerprint(cwd);
  const approval = findApproval(fp);
  if (!approval) return { created: false, reason: 'no-awaiting-approval' };
  const resolved = resolveApproval(fp, { key: approval.key, decision: 'create', now });
  // Approval is now GENERATING. If anything below throws, restore it to
  // AWAITING_USER so the user can retry instead of losing the approval with no
  // capsule to show for it.
  try {
    const context = resolved.context;
    const semantic = typeof sentinel.goal === 'string' && sentinel.goal.trim();
    const payload = semantic ? sentinel : {
      goal: `approved checkpoint at ${context.reading?.usedPercent ?? 'unknown'}%`,
      next_actions: [], status: 'in_progress',
    };
    const { capsule } = buildCheckpointCapsule({
      sentinel: payload,
      cwd: context.cwd || cwd,
      agent: context.agent,
      sessionId: context.sessionId,
      checkpointKey: approval.key,
      now,
      trigger: {
        type: 'rate_limit',
        threshold_percent: context.threshold,
        observed_percent: context.reading?.usedPercent,
        measurement_source: context.reading?.source,
      },
    });
    publishCapsule(fp, capsule, { status: semantic ? 'AVAILABLE' : 'DEGRADED_AVAILABLE', now });
    appendHistory(fp, { event: 'created_from_approval', taskId: capsule.task_id, agent: context.agent }, { now });
    return { created: true, taskId: capsule.task_id, fingerprint: fp, degraded: !semantic };
  } catch (err) {
    // Best-effort recovery: a failure restoring the approval must never replace
    // (mask) the original error the caller needs to see. Always rethrow `err`.
    try { restoreApprovalForRetry(fp, { key: approval.key, now }); } catch {}
    throw err;
  }
}

export function skipApproval({ cwd, now = Date.now() }) {
  const fp = projectFingerprint(cwd);
  const approval = findApproval(fp);
  if (!approval) return { skipped: false, reason: 'no-awaiting-approval' };
  resolveApproval(fp, { key: approval.key, decision: 'skip', now });
  appendHistory(fp, { event: 'skipped', key: approval.key }, { now });
  return { skipped: true, fingerprint: fp };
}

export function previewFor(cwd) {
  const fp = projectFingerprint(cwd);
  const p = findPendingCapsule(fp);
  if (!p || !p.capsule) return { pending: false };
  const verified = verifyStoredCapsule(fp, p.taskId);
  if (!verified.valid) return { pending: true, valid: false, taskId: p.taskId, errors: verified.errors };
  const c = verified.capsule;
  const task = c.task || {};
  return {
    pending: true,
    valid: true,
    taskId: p.taskId,
    goal: task.goal,
    source: c.source && c.source.agent,
    next_actions: task.next_actions || [],
    completed: task.completed || [],
    open_issues: task.open_issues || [],
    changed_files: task.changed_files || [],
  };
}

function scanOtherPending(currentFp, { now = Date.now() } = {}) {
  const projects = join(dataRoot(), 'projects');
  const out = [];
  let names = [];
  try { names = readdirSync(projects); } catch { return out; }
  for (const fp of names) {
    if (fp === currentFp) continue;
    const hdir = join(projects, fp, 'handoff');
    let tasks = [];
    try { tasks = readdirSync(hdir); } catch { continue; }
    for (const taskId of tasks) {
      const statePath = join(hdir, taskId, 'state.json');
      const capPath = join(hdir, taskId, 'capsule.json');
      if (!existsSync(statePath) || !existsSync(capPath)) continue;
      let state; let cap;
      try { state = JSON.parse(readFileSync(statePath, 'utf8')); cap = JSON.parse(readFileSync(capPath, 'utf8')); }
      catch { continue; }
      if (state.status !== 'AVAILABLE' && state.status !== 'DEGRADED_AVAILABLE') continue;
      // A capsule past its TTL is effectively dead — do not advertise it as a
      // pending handoff sitting under another fingerprint (matches recentCapsules).
      if (cap.expires_at && Date.parse(cap.expires_at) <= now) continue;
      out.push({
        fingerprint: fp, taskId,
        goal: cap.task && cap.task.goal,
        source: cap.source && cap.source.agent,
        branch: cap.project && cap.project.git_branch,
      });
    }
  }
  return out;
}

// Newest-first view of capsules across every project bucket. SessionStart and
// findPendingCapsule are per-fingerprint, so this is the only place that answers
// "what handoffs exist globally, regardless of which project I'm in". Read-only:
// it never claims, expires, or recovers a capsule. `current` flags the bucket of
// `currentFingerprint` so a caller can tell "this project" from the rest.
export function recentCapsules({ limit = 10, currentFingerprint = null, now = Date.now() } = {}) {
  const projects = join(dataRoot(), 'projects');
  const out = [];
  let names = [];
  try { names = readdirSync(projects); } catch { return out; }
  for (const fp of names) {
    const hdir = join(projects, fp, 'handoff');
    let tasks = [];
    try { tasks = readdirSync(hdir); } catch { continue; }
    for (const taskId of tasks) {
      const statePath = join(hdir, taskId, 'state.json');
      const capPath = join(hdir, taskId, 'capsule.json');
      if (!existsSync(statePath) || !existsSync(capPath)) continue;
      let state; let cap;
      try { state = JSON.parse(readFileSync(statePath, 'utf8')); cap = JSON.parse(readFileSync(capPath, 'utf8')); }
      catch { continue; }
      const sortMs = Date.parse(cap.created_at) || (typeof state.updated_at === 'number' ? state.updated_at : 0);
      // Report the effective status: a pending capsule past its TTL is shown as
      // EXPIRED even before findPendingCapsule persists the transition, so recent
      // never advertises a dead capsule as AVAILABLE. Read-only — no mutation.
      let status = state.status;
      if ((status === 'AVAILABLE' || status === 'DEGRADED_AVAILABLE')
        && cap.expires_at && Date.parse(cap.expires_at) <= now) {
        status = 'EXPIRED';
      }
      const row = {
        fingerprint: fp,
        taskId,
        status,
        source: cap.source && cap.source.agent,
        target: cap.target && cap.target.agent,
        goal: cap.task && cap.task.goal,
        branch: cap.project && cap.project.git_branch,
        created_at: cap.created_at || null,
        sortMs,
      };
      if (currentFingerprint) row.current = fp === currentFingerprint;
      out.push(row);
    }
  }
  out.sort((a, b) => b.sortMs - a.sortMs);
  return out.slice(0, Math.max(0, limit)).map(({ sortMs, ...rest }) => rest);
}

// Read-only audit of every task directory in this project's bucket. doctor used
// to verify ONLY the selected pending capsule, so a corrupt state.json, an
// orphaned task dir, or a missing sha was reported as healthy. This walks each
// dir and distinguishes the failure modes instead of collapsing a parse error
// into an empty state. Never rewrites or deletes — diagnosis only.
function auditBucket(fingerprint) {
  const hd = handoffDir(fingerprint);
  const findings = [];
  let names = [];
  try { names = readdirSync(hd); } catch { return findings; }
  for (const taskId of names) {
    const dir = join(hd, taskId);
    try { if (!statSync(dir).isDirectory()) continue; } catch { continue; }
    const hasCap = existsSync(join(dir, 'capsule.json'));
    const hasState = existsSync(join(dir, 'state.json'));
    const hasSha = existsSync(join(dir, 'capsule.sha256'));
    if (!hasCap && !hasState) { findings.push({ taskId, issue: 'empty-task-dir' }); continue; }
    if (hasCap && !hasState) findings.push({ taskId, issue: 'missing-state' });
    if (hasState && !hasCap) findings.push({ taskId, issue: 'missing-capsule' });
    if (hasCap && !hasSha) findings.push({ taskId, issue: 'missing-sha' });
    if (hasState) {
      let parsed; let parseOk = true;
      try { parsed = JSON.parse(readFileSync(join(dir, 'state.json'), 'utf8')); }
      catch { parseOk = false; }
      if (!parseOk) findings.push({ taskId, issue: 'invalid-state-json' });
      // Parses but is not a usable state object (literal null, an array, or no
      // status string) — would read as {} elsewhere and hide a corrupt file.
      else if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed) || typeof parsed.status !== 'string') {
        findings.push({ taskId, issue: 'invalid-state-shape' });
      }
    }
  }
  return findings;
}

// History rows whose task directory no longer exists. Nothing in the runtime
// deletes a task dir, so a `created` event with no dir is a real anomaly worth
// surfacing (it was the unexplained disappearance seen during review).
function danglingHistory(fingerprint) {
  const hd = handoffDir(fingerprint);
  const out = [];
  const seen = new Set();
  for (const ev of readHistory(fingerprint, { limit: 500 })) {
    const taskId = ev && ev.taskId;
    if (!taskId || seen.has(taskId)) continue;
    seen.add(taskId);
    if (!existsSync(join(hd, taskId))) out.push({ taskId, issue: 'history-without-task', event: ev.event });
  }
  return out;
}

export function doctorFor(cwd, { now = Date.now() } = {}) {
  const { fingerprint, basis } = projectFingerprintInfo(cwd);
  let cwdResolved = cwd;
  try { cwdResolved = realpathSync(cwd); } catch {}
  const pending = findPendingCapsule(fingerprint, { now });
  const approval = findApproval(fingerprint);
  const issues = [];
  let verified = null;
  if (pending?.capsule) {
    verified = verifyStoredCapsule(fingerprint, pending.taskId, { now });
    issues.push(...verified.errors);
  }
  const audit = [...auditBucket(fingerprint), ...danglingHistory(fingerprint)];
  return {
    fingerprint,
    basis,
    cwdResolved,
    dataRoot: dataRoot(),
    healthy: issues.length === 0 && audit.length === 0,
    issues,
    audit,
    pending: pending ? {
      taskId: pending.taskId,
      status: pending.state.status,
      recoveredAt: pending.state.recovered_at || null,
      verified: verified?.valid ?? false,
    } : null,
    approval: approval ? { key: approval.key, status: approval.status } : null,
    otherPending: scanOtherPending(fingerprint, { now }),
    stateFiles: stateReport(fingerprint),
  };
}
