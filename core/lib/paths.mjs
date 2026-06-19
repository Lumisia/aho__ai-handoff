import { homedir } from 'node:os';
import { join } from 'node:path';

export function dataRoot() {
  if (process.env.AI_HANDOFF_ROOT) return process.env.AI_HANDOFF_ROOT;
  const home = homedir();
  if (process.platform === 'win32') {
    return join(process.env.LOCALAPPDATA || join(home, 'AppData', 'Local'), 'ai-handoff');
  }
  if (process.platform === 'darwin') {
    return join(home, 'Library', 'Application Support', 'ai-handoff');
  }
  return join(process.env.XDG_STATE_HOME || join(home, '.local', 'state'), 'ai-handoff');
}

export function configPath() { return join(dataRoot(), 'config.json'); }
export function globalStatePath() { return join(dataRoot(), 'state.json'); }
export function projectDir(fingerprint) { return join(dataRoot(), 'projects', fingerprint); }
export function handoffDir(fingerprint) { return join(projectDir(fingerprint), 'handoff'); }
export function claudeRateLimitDir() { return join(dataRoot(), 'sensors', 'claude'); }
export function claudeStatuslineStatePath() { return join(dataRoot(), 'claude-statusline.json'); }
export function memoryDir(fingerprint) { return join(projectDir(fingerprint), 'memory'); }
export function memoryRecallStatePath() { return join(dataRoot(), 'memory-recall-state.json'); }
