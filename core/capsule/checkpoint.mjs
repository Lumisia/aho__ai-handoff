import { gitContext } from '../lib/gitctx.mjs';
import { projectFingerprint } from '../lib/fingerprint.mjs';
import { instanceKey, deriveTaskId } from '../lib/taskid.mjs';
import { buildCapsule } from './create.mjs';
import { redactJson } from '../lib/redact.mjs';

export function buildCheckpointCapsule({
  sentinel = {}, cwd, agent, sessionId, checkpointKey, now = Date.now(),
  ttlMs = 24 * 60 * 60 * 1000, trigger,
} = {}) {
  const fp = projectFingerprint(cwd);
  const git = gitContext(cwd);
  const taskRaw = {
    goal: sentinel.goal || 'manual checkpoint',
    next_actions: sentinel.next_actions || [],
    completed: sentinel.completed || [],
    open_issues: sentinel.open_issues || [],
  };
  const { value: task, count } = redactJson(taskRaw);
  const taskId = deriveTaskId({
    projectFingerprint: fp,
    instanceKey: instanceKey({ explicitTaskKey: checkpointKey, agent, sessionId }),
    goalSlug: task.goal,
  });
  const capsule = buildCapsule({
    taskId,
    now: new Date(now).toISOString(),
    expiresAt: new Date(now + ttlMs).toISOString(),
    source: { agent, session_id: sessionId },
    target: { agent: agent === 'codex' ? 'claude-code' : 'codex' },
    trigger: trigger || { type: 'manual_checkpoint' },
    project: { fingerprint: fp, git_branch: git.branch, git_head: git.head, working_tree_dirty: git.dirty },
    checkpoint: { status: sentinel.status || 'in_progress' },
    task,
    security: { redactions_applied: count },
  });
  return { capsule, fingerprint: fp };
}
