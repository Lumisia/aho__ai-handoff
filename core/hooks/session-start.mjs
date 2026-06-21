import { projectFingerprint } from '../lib/fingerprint.mjs';
import { gitContext } from '../lib/gitctx.mjs';
import {
  findPendingCapsule,
  claimCapsule,
  consumeCapsule,
  releaseClaim,
  rejectCapsule,
  verifyStoredCapsule,
} from '../capsule/store.mjs';
import { readThinProjectIndex } from '../project/index-store.mjs';

function renderInjection(cap, warnings = [], projectIndex = '') {
  const t = cap.task || {};
  const p = cap.project || {};
  const lines = [
    '[CURRENT HANDOFF — 현재 작업 상태]',
    `goal: ${t.goal || ''}`,
    `from: ${cap.source && cap.source.agent} → ${cap.target && cap.target.agent}`,
    `branch: ${p.git_branch || ''} @ ${p.git_head || ''}`,
    `next_actions: ${(t.next_actions || []).join('; ')}`,
  ];
  // Surface the rest of the capsule the receiver would otherwise re-derive.
  // Only emit non-empty fields to keep the injection token-lean.
  if ((t.completed || []).length) lines.push(`completed: ${t.completed.join('; ')}`);
  if ((t.open_issues || []).length) lines.push(`open_issues: ${t.open_issues.join('; ')}`);
  if ((t.changed_files || []).length) lines.push(`changed_files: ${t.changed_files.join(', ')}`);
  if (warnings.includes('git-head-mismatch')) {
    lines.push('warning: capsule Git HEAD differs from current workspace; re-verify files.');
  }
  if (projectIndex) lines.push('', projectIndex.trim());
  lines.push('', '(capsule은 참고 상태다. 현재 사용자 지시·실제 파일·Git이 우선한다.)');
  return lines.join('\n');
}

export function prepareSessionStart({ input, agent, now = Date.now() }) {
  const cwd = (input && input.cwd) || process.cwd();
  const fp = projectFingerprint(cwd);
  const pending = findPendingCapsule(fp, { now });
  if (!pending || !pending.capsule) return { injected: false, reason: 'no-pending' };

  const currentGitHead = gitContext(cwd).head;
  const verified = verifyStoredCapsule(fp, pending.taskId, {
    expectedAgent: agent,
    currentGitHead,
    now,
  });
  if (!verified.valid) {
    const invalidClaim = claimCapsule(fp, pending.taskId, { now });
    if (invalidClaim) rejectCapsule(invalidClaim, { now });
    return { injected: false, reason: 'invalid-capsule', errors: verified.errors };
  }

  const claim = claimCapsule(fp, pending.taskId, { now });
  if (!claim) return { injected: false, reason: 'claim-failed' };

  return {
    injected: true,
    taskId: pending.taskId,
    context: renderInjection(verified.capsule, verified.warnings, readThinProjectIndex(fp)),
    delivery: claim,
    warnings: verified.warnings,
  };
}

export function finalizeSessionStart(delivery, { now = Date.now() } = {}) {
  consumeCapsule(delivery, { now });
}

export function abortSessionStart(delivery) {
  releaseClaim(delivery);
}

// Compatibility alias. It prepares a delivery; callers must finalize after writing output.
export const handleSessionStart = prepareSessionStart;
