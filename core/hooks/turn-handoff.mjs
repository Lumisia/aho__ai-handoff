import { projectFingerprint } from '../lib/fingerprint.mjs';
import { resolveProject } from '../lib/config.mjs';
import { evaluateTrigger } from './trigger.mjs';
import { dedupeKey, hasSeen } from '../lib/dedupe.mjs';
import { globalStatePath } from '../lib/paths.mjs';
import { readState } from '../capsule/store.mjs';
import { readSamples, appendSample } from '../sensors/samples.mjs';
import { saveApproval, findApproval } from '../capsule/approval.mjs';
import {
  findGenerationForTurn,
  generationSlotKey,
  saveGeneration,
} from '../capsule/generation.mjs';
import {
  codexAskInstruction,
  codexAutoFooterInstruction,
} from '../lib/inline-capsule.mjs';

export async function prepareTurnHandoff({
  input = {},
  config,
  readSensor,
  agent,
  now = Date.now(),
} = {}) {
  if (agent !== 'codex') return { injected: false, reason: 'non-codex-agent' };

  const cwd = input.cwd || process.cwd();
  const fp = projectFingerprint(cwd);
  const pcfg = resolveProject(config, fp);
  const tcfg = pcfg.triggers?.five_hour || {};
  const codexCfg = pcfg.codex || {};
  if (tcfg.enabled === false) return { injected: false, reason: 'disabled', fingerprint: fp };

  const reading = await readSensor();
  if (reading && typeof reading.usedPercent === 'number') {
    appendSample(fp, agent, { usedPercent: reading.usedPercent, at: now });
  }

  const gstate = readState(globalStatePath());
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
    burnRate: tcfg.burn_rate && {
      enabled: tcfg.burn_rate.enabled,
      runwayMinutes: tcfg.burn_rate.runway_minutes,
    },
    now,
  });

  if (ev.action === 'none') {
    return { injected: false, reason: ev.reason, fingerprint: fp };
  }

  const slotKey = generationSlotKey({
    agent,
    sessionId: input.session_id,
    projectFingerprint: fp,
    turnId: input.turn_id || null,
  });

  if (ev.action === 'create') {
    if (codexCfg.inline_final_capsule === false) {
      return { injected: false, reason: 'inline-final-disabled', fingerprint: fp };
    }
    if (findGenerationForTurn({
      agent,
      sessionId: input.session_id,
      projectFingerprint: fp,
      turnId: input.turn_id || null,
    })) {
      return { injected: false, reason: 'already-generating', fingerprint: fp, slotKey };
    }

    saveGeneration({
      slotKey,
      now,
      context: {
        strategy: 'codex-inline-final',
        agent,
        sessionId: input.session_id,
        turnId: input.turn_id || null,
        projectFingerprint: fp,
        cwd,
        reading,
        threshold: tcfg.threshold_percent,
        dedupeKey: dkey,
      },
    });

    return {
      injected: true,
      action: 'create',
      reason: ev.reason,
      fingerprint: fp,
      slotKey,
      context: codexAutoFooterInstruction({
        usedPercent: reading?.usedPercent,
        thresholdPercent: tcfg.threshold_percent,
        locale: pcfg.locale || config.locale || 'en',
      }),
    };
  }

  if (ev.action === 'ask') {
    const existingApproval = findApproval(fp, { key: dkey, now });
    if (existingApproval?.context?.deliveredByTurnHandoff === true) {
      return { injected: false, reason: 'already-awaiting-user', fingerprint: fp };
    }

    saveApproval({
      fingerprint: fp,
      key: dkey,
      now,
      context: {
        agent,
        sessionId: input.session_id,
        turnId: input.turn_id || null,
        deliveredByTurnHandoff: true,
        cwd,
        reading,
        threshold: tcfg.threshold_percent,
      },
      ttlMs: pcfg.approval?.ttl_ms,
    });

    return {
      injected: true,
      action: 'ask',
      reason: ev.reason,
      fingerprint: fp,
      slotKey,
      approvalKey: dkey,
      context: codexAskInstruction({ locale: pcfg.locale || config.locale || 'en' }),
    };
  }

  return { injected: false, reason: ev.reason, fingerprint: fp };
}
