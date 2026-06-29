export type CheckStatus = "ok" | "warning" | "error" | "missing" | "unknown";

export interface CheckRow {
  id: string;
  label: string;
  status: CheckStatus;
  message: string;
  path?: string | null;
}

export interface DashboardPaths {
  ai_home: string;
  ipc: string;
  store: string;
  logs: string;
  install_state: string;
  codex_hooks: string;
  codex_config: string;
  claude_settings: string;
}

export interface InstallSummary {
  status: CheckStatus;
  version: number;
  installed_at: string;
  autostart: string;
  launcher?: string | null;
}

export interface CapsuleSummary {
  capsule_id: string;
  project_id: string;
  project_label: string;
  created_at: string;
  source_agent: string;
  target_agent: string;
  state: string;
  summary_preview: string;
  path: string;
}

export interface CapsuleList {
  items: CapsuleSummary[];
  pending_count: number;
  skipped: number;
}

export interface ReadTextResult {
  path: string;
  text: string;
  truncated: boolean;
  error?: string | null;
}

export interface LogFile {
  name: string;
  result: ReadTextResult;
}

export interface DashboardSnapshot {
  paths: DashboardPaths;
  install_state: InstallSummary;
  daemon: CheckRow;
  autostart: CheckRow;
  codex_hooks: CheckRow;
  codex_config: CheckRow;
  claude_settings: CheckRow;
  ipc: CheckRow;
  store: CheckRow;
  duplicates: CheckRow[];
  capsules: CapsuleList;
  checks: CheckRow[];
}
