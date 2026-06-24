import { appendFileSync } from 'node:fs';
import { join } from 'node:path';
import { projectFingerprint } from '../lib/fingerprint.mjs';
import { evaluateTrigger } from './trigger.mjs';
import { resolveProject } from '../lib/config.mjs';
import { publishCapsule, readState, writeState } from '../capsule/store.mjs';
import { dedupeKey, hasSeen, markSeen } from '../lib/dedupe.mjs';
import { dataRoot, globalStatePath } from '../lib/paths.mjs';

// Opt-in trace of every Stop decision, written to <dataRoot>/stop-debug.log, so
// a live "why didn't it ask?" can be diagnosed from ground truth instead of
// guesswork. Off unless config.debug.stop_log (or AI_HANDOFF_STOP_DEBUG) is set.
function stopDebugLog(enabled, entry) {
  if (!enabled) return;
  try { appendFileSync(join(dataRoot(), 'stop-debug.log'), JSON.stringify(entry) + '\n'); } catch {}
}
import { saveApproval } from '../capsule/approval.mjs';
import { sendNotification } from '../lib/notify.mjs';
import {
  generationSlotKey, saveGeneration, findGeneration, findGenerationForTurn, finishGeneration,
} from '../capsule/generation.mjs';
import { buildCheckpointCapsule } from '../capsule/checkpoint.mjs';
import { appendSample, readSamples } from '../sensors/samples.mjs';
import { t } from '../lib/i18n.mjs';
import {
  extractInlineCapsule,
  fallbackInlineSentinel,
} from '../lib/inline-capsule.mjs';

function extractSentinel(text) {
  const match = String(text || '').match(/<handoff-capsule>\s*([\s\S]*?)\s*<\/handoff-capsule>/i);
  if (!match) return null;
  try {
    const value = JSON.parse(match[1]);
    return value && typeof value.goal === 'string' && value.goal.trim() ? value : null;
  } catch { return null; }
}

export async function handleStop({ input, config, readSensor, agent, now = Date.now(), notifyFn = sendNotification }) {
  const cwd = input.cwd || process.cwd();
  const fp = projectFingerprint(cwd);
  const pcfg = resolveProject(config, fp);
  const tcfg = pcfg.triggers.five_hour;
  const locale = pcfg.locale || 'en';
  const notification = pcfg.notification || {};
  const noticeMethod = notification.method ?? 'os';
  const noticeOpts = { method: noticeMethod, fallback: notification.fallback ?? 'terminal' };
  const sendNotice = (title, body) => { if (noticeMethod !== 'off') notifyFn(title, body, noticeOpts); };
  const debugStop = !!(config.debug?.stop_log) || !!(pcfg.debug?.stop_log) || !!process.env.AI_HANDOFF_STOP_DEBUG;
  const slotKey = generationSlotKey({
    agent,
    sessionId: input.session_id,
    projectFingerprint: fp,
    turnId: input.turn_id || null,
  });

  const locatedGeneration = findGenerationForTurn({
    agent,
    sessionId: input.session_id,
    projectFingerprint: fp,
    turnId: input.turn_id || null,
  });
  const generation = locatedGeneration?.generation || null;
  const activeSlotKey = locatedGeneration?.slotKey || slotKey;
  if (
    agent === 'codex'
    && generation?.context?.strategy === 'codex-inline-final'
  ) {
    const context = generation.context;
    const semantic = extractInlineCapsule(input.last_assistant_message);
    const degraded = !semantic;
    const sentinel = semantic || fallbackInlineSentinel({
      reading: context.reading,
      threshold: context.threshold,
    });

    const { capsule } = buildCheckpointCapsule({
      sentinel,
      cwd: context.cwd,
      agent: context.agent,
      sessionId: context.sessionId,
      checkpointKey: context.dedupeKey,
      now,
      trigger: {
        type: 'rate_limit',
        threshold_percent: context.threshold,
        observed_percent: context.reading?.usedPercent,
        measurement_source: context.reading?.source,
      },
    });

    publishCapsule(fp, capsule, { status: degraded ? 'DEGRADED_AVAILABLE' : 'AVAILABLE', now });
    const gpath = globalStatePath();
    writeState(gpath, markSeen(readState(gpath), context.dedupeKey, now));
    finishGeneration(activeSlotKey, { now });
    sendNotice('AI handoff', t('notify.capsule_ready', { agent: capsule.target.agent }, locale));
    return {
      action: 'create',
      reason: degraded ? 'codex-inline-final-degraded' : 'codex-inline-final',
      taskId: capsule.task_id,
      fingerprint: fp,
      degraded,
    };
  }

  if (input.stop_hook_active) {
    if (!generation) return { action: 'none', reason: 'no-generation', fingerprint: fp };
    const context = generation.context;
    const semantic = extractSentinel(input.last_assistant_message);
    const degraded = !semantic;
    const sentinel = semantic || {
      goal: `auto checkpoint at ${context.reading?.usedPercent ?? 'unknown'}%`,
      next_actions: [], completed: [], open_issues: [], status: 'in_progress',
    };
    const { capsule } = buildCheckpointCapsule({
      sentinel,
      cwd: context.cwd,
      agent: context.agent,
      sessionId: context.sessionId,
      checkpointKey: context.dedupeKey,
      now,
      trigger: {
        type: 'rate_limit',
        threshold_percent: context.threshold,
        observed_percent: context.reading?.usedPercent,
        measurement_source: context.reading?.source,
      },
    });
    publishCapsule(fp, capsule, { status: degraded ? 'DEGRADED_AVAILABLE' : 'AVAILABLE', now });
    const gpath = globalStatePath();
    writeState(gpath, markSeen(readState(gpath), context.dedupeKey, now));
    finishGeneration(activeSlotKey, { now });
    sendNotice('AI handoff', t('notify.capsule_ready', { agent: capsule.target.agent }, locale));
    return { action: 'create', reason: 'threshold', taskId: capsule.task_id, fingerprint: fp, degraded };
  }

  if (tcfg.enabled === false) return { action: 'none', reason: 'disabled', fingerprint: fp };

  const reading = await readSensor();
  if (reading && typeof reading.usedPercent === 'number') {
    appendSample(fp, agent, { usedPercent: reading.usedPercent, at: now });
  }
  const gpath = globalStatePath();
  const gstate = readState(gpath);
  const dkey = dedupeKey({
    source: agent,
    windowDuration: reading && reading.windowMinutes,
    resetsAt: reading && reading.resetsAt,
    sessionId: input.session_id,
    projectFingerprint: fp,
    threshold: tcfg.threshold_percent,
  });
  const ev = evaluateTrigger({
    usedPercent: reading && reading.usedPercent,
    threshold: tcfg.threshold_percent,
    mode: tcfg.mode,
    deduped: hasSeen(gstate, dkey),
    samples: readSamples(fp, agent),
    burnRate: tcfg.burn_rate && { enabled: tcfg.burn_rate.enabled, runwayMinutes: tcfg.burn_rate.runway_minutes },
    now,
  });
  stopDebugLog(debugStop, {
    at: new Date(now).toISOString(),
    agent, sessionId: input.session_id, fingerprint: fp,
    mode: tcfg.mode, threshold: tcfg.threshold_percent,
    sensor: reading ? { usedPercent: reading.usedPercent, source: reading.source, capturedAt: reading.capturedAt } : null,
    deduped: hasSeen(gstate, dkey),
    action: ev.action, reason: ev.reason,
  });
  if (ev.action === 'none') return { action: 'none', reason: ev.reason, fingerprint: fp };

  if (ev.action === 'ask') {
    // Do NOT mark the window seen here. Asking is not resolving: if the model
    // fails to surface the picker, or the user never answers, a later Stop must
    // be free to ask again. The window is marked seen only once the user
    // actually creates or skips the capsule (see core/hooks/handoff.mjs).
    saveApproval({
      fingerprint: fp,
      key: dkey,
      now,
      context: {
        agent,
        sessionId: input.session_id,
        turnId: input.turn_id || null,
        projectFingerprint: fp,
        cwd,
        reading,
        threshold: tcfg.threshold_percent,
      },
      ttlMs: pcfg.approval?.ttl_ms,
    });
    sendNotice('AI handoff', t('ask.create_or_skip', {}, locale));
    if (
      agent === 'codex'
      && pcfg.codex?.stop_continuation_ask !== true
      && config.codex?.stop_continuation_ask !== true
    ) {
      return {
        action: 'none',
        reason: 'codex-ask-deferred-to-next-turn-context',
        fingerprint: fp,
        approvalKey: dkey,
      };
    }
    return { action: 'ask', reason: ev.reason, fingerprint: fp, approvalKey: dkey };
  }

  if (
    agent === 'codex'
    && pcfg.codex?.stop_continuation_auto_summary !== true
    && config.codex?.stop_continuation_auto_summary !== true
  ) {
    if (pcfg.codex?.degraded_fallback_on_stop === false || config.codex?.degraded_fallback_on_stop === false) {
      return { action: 'none', reason: 'codex-stop-continuation-disabled', fingerprint: fp };
    }

    const sentinel = fallbackInlineSentinel({
      reading,
      threshold: tcfg.threshold_percent,
    });
    const { capsule } = buildCheckpointCapsule({
      sentinel,
      cwd,
      agent,
      sessionId: input.session_id,
      checkpointKey: dkey,
      now,
      trigger: {
        type: 'rate_limit',
        threshold_percent: tcfg.threshold_percent,
        observed_percent: reading?.usedPercent,
        measurement_source: reading?.source,
      },
    });

    publishCapsule(fp, capsule, { status: 'DEGRADED_AVAILABLE', now });
    writeState(gpath, markSeen(readState(gpath), dkey, now));
    sendNotice('AI handoff', t('notify.capsule_ready', { agent: capsule.target.agent }, locale));
    return {
      action: 'create',
      reason: 'codex-stop-degraded-no-continuation',
      taskId: capsule.task_id,
      fingerprint: fp,
      degraded: true,
    };
  }

  saveGeneration({
    slotKey,
    now,
    context: {
      strategy: 'stop-continuation',
      agent, sessionId: input.session_id, cwd, reading,
      projectFingerprint: fp,
      threshold: tcfg.threshold_percent, dedupeKey: dkey,
    },
  });
  return { action: 'request-summary', reason: ev.reason, fingerprint: fp, prompt: t('summary.instruction', {}, locale) };
}
