import { projectFingerprint } from '../lib/fingerprint.mjs';
import { gitContext } from '../lib/gitctx.mjs';
import {
  findPendingCapsule,
  claimCapsule,
  rejectCapsule,
  verifyStoredCapsule,
} from '../capsule/store.mjs';
import { recordInject } from '../capsule/inject-track.mjs';
import { appendHistory } from '../capsule/history.mjs';
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

  // A capsule is addressed to exactly one agent. If it targets the peer agent,
  // leave it untouched — do NOT verify or reject it, or this agent would destroy
  // a handoff meant for its peer (a peer-targeted capsule fails the expectedAgent
  // check and would otherwise be claimed+rejected).
  if (pending.capsule.target?.agent !== agent) {
    return { injected: false, reason: 'not-target-agent' };
  }

  const currentGitHead = gitContext(cwd).head;
  const verified = verifyStoredCapsule(fp, pending.taskId, {
    expectedAgent: agent,
    currentGitHead,
    now,
  });
  if (!verified.valid) {
    // A tampered/invalid capsule is cleaned up (claim then reject) even on the
    // read-only inject path — we never inject unverified content.
    const invalidClaim = claimCapsule(fp, pending.taskId, { now });
    if (invalidClaim) rejectCapsule(invalidClaim, { now });
    return { injected: false, reason: 'invalid-capsule', errors: verified.errors };
  }

  // Inject is read-only: the capsule status is NOT changed here. Consuming it
  // would lose the handoff to an ephemeral session whose SessionStart hook ran
  // but which never reached a prompt. The capsule is consumed only once the
  // session proves it is live (its first UserPromptSubmit, via consumeOnPrompt).
  return {
    injected: true,
    taskId: pending.taskId,
    context: renderInjection(verified.capsule, verified.warnings, readThinProjectIndex(fp)),
    warnings: verified.warnings,
    delivery: { fingerprint: fp, taskId: pending.taskId, sessionId: input && input.session_id, agent },
  };
}

// Record the injection so the session's first prompt can consume the capsule.
// This deliberately does NOT consume: a SessionStart that never reaches a prompt
// leaves the capsule pending for the next session.
export function finalizeSessionStart(delivery, { now = Date.now() } = {}) {
  recordInject({ fingerprint: delivery.fingerprint, sessionId: delivery.sessionId, taskId: delivery.taskId, now });
  appendHistory(delivery.fingerprint, {
    event: 'injected', taskId: delivery.taskId, agent: delivery.agent, session_id: delivery.sessionId,
  }, { now });
}

// Output delivery failed after prepare. Inject is read-only until finalize, so
// nothing was claimed or recorded and there is nothing to roll back.
export function abortSessionStart() { /* no-op */ }

// Compatibility alias. It prepares a delivery; callers must finalize after writing output.
export const handleSessionStart = prepareSessionStart;
