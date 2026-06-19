import { projectFingerprint } from '../lib/fingerprint.mjs';
import { findPendingCapsule, verifyStoredCapsule } from '../capsule/store.mjs';
import { publishCapsule } from '../capsule/store.mjs';
import { findApproval, resolveApproval } from '../capsule/approval.mjs';
import { buildCheckpointCapsule } from '../capsule/checkpoint.mjs';

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
  return { created: true, taskId: capsule.task_id, fingerprint: fp, degraded: !semantic };
}

export function skipApproval({ cwd, now = Date.now() }) {
  const fp = projectFingerprint(cwd);
  const approval = findApproval(fp);
  if (!approval) return { skipped: false, reason: 'no-awaiting-approval' };
  resolveApproval(fp, { key: approval.key, decision: 'skip', now });
  return { skipped: true, fingerprint: fp };
}

export function previewFor(cwd) {
  const fp = projectFingerprint(cwd);
  const p = findPendingCapsule(fp);
  if (!p || !p.capsule) return { pending: false };
  const verified = verifyStoredCapsule(fp, p.taskId);
  if (!verified.valid) return { pending: true, valid: false, taskId: p.taskId, errors: verified.errors };
  const c = verified.capsule;
  return {
    pending: true,
    valid: true,
    taskId: p.taskId,
    goal: c.task && c.task.goal,
    source: c.source && c.source.agent,
    next_actions: (c.task && c.task.next_actions) || [],
  };
}

export function recoverFor(cwd, { now = Date.now() } = {}) {
  const fingerprint = projectFingerprint(cwd);
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
    healthy: issues.length === 0,
    issues,
    pending: pending ? {
      taskId: pending.taskId,
      status: pending.state.status,
      recoveredAt: pending.state.recovered_at || null,
      verified: verified?.valid ?? false,
    } : null,
    approval: approval ? { key: approval.key, status: approval.status } : null,
  };
}
