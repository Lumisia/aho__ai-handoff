import { projectFingerprint, projectFingerprintInfo } from '../lib/fingerprint.mjs';
import { appendHistory } from '../capsule/history.mjs';
import { readdirSync, readFileSync, existsSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { findPendingCapsule, verifyStoredCapsule } from '../capsule/store.mjs';
import { publishCapsule } from '../capsule/store.mjs';
import { findApproval, resolveApproval } from '../capsule/approval.mjs';
import { buildCheckpointCapsule } from '../capsule/checkpoint.mjs';
import { dataRoot } from '../lib/paths.mjs';
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

function scanOtherPending(currentFp) {
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
  return {
    fingerprint,
    basis,
    cwdResolved,
    dataRoot: dataRoot(),
    healthy: issues.length === 0,
    issues,
    pending: pending ? {
      taskId: pending.taskId,
      status: pending.state.status,
      recoveredAt: pending.state.recovered_at || null,
      verified: verified?.valid ?? false,
    } : null,
    approval: approval ? { key: approval.key, status: approval.status } : null,
    otherPending: scanOtherPending(fingerprint),
    stateFiles: stateReport(fingerprint),
  };
}
