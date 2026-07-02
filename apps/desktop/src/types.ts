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

export interface UsageTokens {
  input: number;
  cache_read: number;
  cache_write: number;
  output: number;
  total: number;
}

export interface UsageGroup {
  key: string;
  tokens: UsageTokens;
  cost_usd: number;
  unpriced_tokens: number;
  events: number;
}

export interface UsageReport {
  total: UsageGroup;
  by_source: UsageGroup[];
  by_day: UsageGroup[];
  by_model: UsageGroup[];
  by_project: UsageGroup[];
}

export interface AccountWindow {
  used_percent: number;
  remaining_percent: number;
  window_minutes: number;
  resets_at?: number | null;
}

export interface AccountSlotRow {
  label: string;
  email?: string | null;
  plan?: string | null;
  account_id?: string | null;
  source?: string | null;
  created_at?: string | null;
  active: boolean;
  path: string;
}

export interface AccountAgentReport {
  agent: "codex" | "claude";
  root: string;
  active?: string | null;
  plan?: string | null;
  five_hour?: AccountWindow | null;
  weekly?: AccountWindow | null;
  slots: AccountSlotRow[];
}

export interface AccountReport {
  codex: AccountAgentReport;
  claude: AccountAgentReport;
}

export interface AccountLoginSession {
  agent: "codex" | "claude";
  home: string;
  message: string;
}

export interface AccountLoginPoll {
  done: boolean;
  message: string;
  label?: string | null;
  report?: AccountReport | null;
}

export interface AccountOpResult {
  message: string;
  report: AccountReport;
}

export interface ResetCreditRow {
  granted_at: string;
  expires_at: string;
}

export interface SlotUsageReport {
  plan?: string | null;
  five_hour?: AccountWindow | null;
  weekly?: AccountWindow | null;
  reset_credits?: number | null;
  reset_credit_details: ResetCreditRow[];
}

export interface ThemeReport {
  language: string;
  preset: string;
  codex_color: string;
  claude_color: string;
  focus_border_color: string;
  selection_bg_color: string;
  selection_fg_color: string;
  app_bg_color: string;
  sidebar_bg_color: string;
  panel_bg_color: string;
  text_color: string;
}

export interface DoctorSummary {
  daemon: string;
  ok: number;
  warn: number;
  fail: number;
  codex_accounts: number;
  claude_accounts: number;
  elapsed_ms: number;
  lines: string[];
}

export interface RepairAction {
  id: string;
  label: string;
  detail: string;
  command?: string[] | null;
  requires_confirm: boolean;
  manual: boolean;
  recommended_by: string[];
}

export interface IntegrationReport {
  snapshot: DashboardSnapshot;
  doctor: DoctorSummary;
  repairs: RepairAction[];
}

export interface RepairRunResult {
  action: RepairAction;
  exit_code?: number | null;
  output: string;
  report: IntegrationReport;
}

export interface ConfigRow {
  key: string;
  value: string;
  default_value: string;
  kind: string;
  category: string;
  description: string;
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
