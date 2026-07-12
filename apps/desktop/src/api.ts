import { invoke } from "@tauri-apps/api/core";
import type {
  AccountReport,
  AccountLoginPoll,
  AccountLoginSession,
  AccountOpResult,
  CapsuleList,
  ConfigRow,
  DashboardSnapshot,
  IntegrationReport,
  LimitAlert,
  LogFile,
  ReadTextResult,
  RepairRunResult,
  SlotUsageReport,
  ThemeReport,
  UpdateStatus,
  UsageReport,
} from "./types";

const usageReportCache = new Map<number, UsageReport>();
const usageReportPromises = new Map<number, Promise<UsageReport>>();
let accountReportCache: AccountReport | null = null;
let accountReportPromise: Promise<AccountReport> | null = null;
let configRowsCache: ConfigRow[] | null = null;
let configRowsPromise: Promise<ConfigRow[]> | null = null;
let logsCache: LogFile[] | null = null;
let logsPromise: Promise<LogFile[]> | null = null;
let integrationReportCache: IntegrationReport | null = null;
let dashboardSnapshotCache: DashboardSnapshot | null = null;
let dashboardSnapshotPromise: Promise<DashboardSnapshot> | null = null;
let themeCache: ThemeReport | null = null;
let themePromise: Promise<ThemeReport> | null = null;

interface MenuCommandResult {
  message: string;
}

const DASHBOARD_TIMEOUT_MS = 8000;

function withTimeout<T>(promise: Promise<T>, label: string, timeoutMs: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(`${label} timed out after ${Math.round(timeoutMs / 1000)}s`));
    }, timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer) clearTimeout(timer);
  });
}

function invalidateDashboardCache() {
  dashboardSnapshotCache = null;
  themeCache = null;
}

export function getDashboardSnapshot(options: { force?: boolean } = {}): Promise<DashboardSnapshot> {
  if (!options.force && dashboardSnapshotCache) return Promise.resolve(dashboardSnapshotCache);
  if (dashboardSnapshotPromise) return dashboardSnapshotPromise;
  dashboardSnapshotPromise = withTimeout(
    invoke<DashboardSnapshot>("get_dashboard_snapshot"),
    "Dashboard refresh",
    DASHBOARD_TIMEOUT_MS,
  )
    .then((snapshot) => {
      dashboardSnapshotCache = snapshot;
      return snapshot;
    })
    .finally(() => {
      dashboardSnapshotPromise = null;
    });
  return dashboardSnapshotPromise;
}

export function listCapsules(): Promise<CapsuleList> {
  return invoke("list_capsules");
}

export function readCapsule(path: string): Promise<ReadTextResult> {
  return invoke("read_capsule", { path });
}

export function readLogs(options: { force?: boolean } = {}): Promise<LogFile[]> {
  if (!options.force && logsCache) return Promise.resolve(logsCache);
  if (!options.force && logsPromise) return logsPromise;
  logsPromise = invoke<LogFile[]>("read_logs")
    .then((logs) => {
      logsCache = logs;
      return logs;
    })
    .finally(() => {
      logsPromise = null;
    });
  return logsPromise;
}

export function getUsageReport(
  options: { force?: boolean; days?: number } = {},
): Promise<UsageReport> {
  const days = options.days ?? 30;
  if (!options.force) {
    const cached = usageReportCache.get(days);
    if (cached) return Promise.resolve(cached);
    const inflight = usageReportPromises.get(days);
    if (inflight) return inflight;
  }
  const promise = invoke<UsageReport>("get_usage_report", { days })
    .then((report) => {
      usageReportCache.set(days, report);
      return report;
    })
    .finally(() => {
      usageReportPromises.delete(days);
    });
  usageReportPromises.set(days, promise);
  return promise;
}

function cacheAccountReport(report: AccountReport) {
  accountReportCache = report;
  return report;
}

export function getAccountReport(options: { force?: boolean } = {}): Promise<AccountReport> {
  if (!options.force && accountReportCache) return Promise.resolve(accountReportCache);
  if (!options.force && accountReportPromise) return accountReportPromise;
  accountReportPromise = invoke<AccountReport>("get_account_report", { force: options.force ?? false })
    .then(cacheAccountReport)
    .finally(() => {
      accountReportPromise = null;
    });
  return accountReportPromise;
}

export function startAccountLogin(agent: "codex" | "claude"): Promise<AccountLoginSession> {
  return invoke("start_account_login", { agent });
}

export function pollAccountLogin(agent: "codex" | "claude", home: string): Promise<AccountLoginPoll> {
  return invoke<AccountLoginPoll>("poll_account_login", { agent, home }).then((poll) => {
    if (poll.report) cacheAccountReport(poll.report);
    return poll;
  });
}

export function launchAccountSlot(agent: "codex" | "claude", label: string): Promise<AccountOpResult> {
  return invoke<AccountOpResult>("launch_account_slot", { agent, label }).then((result) => {
    cacheAccountReport(result.report);
    return result;
  });
}

export function refreshAccountSlotUsage(
  agent: "codex" | "claude",
  label: string,
): Promise<SlotUsageReport> {
  return invoke("refresh_account_slot_usage", { agent, label });
}

export function getTheme(options: { force?: boolean } = {}): Promise<ThemeReport> {
  if (!options.force && themeCache) return Promise.resolve(themeCache);
  if (themePromise) return themePromise;
  themePromise = withTimeout(invoke<ThemeReport>("get_theme"), "Theme refresh", DASHBOARD_TIMEOUT_MS)
    .then((theme) => {
      themeCache = theme;
      return theme;
    })
    .finally(() => {
      themePromise = null;
    });
  return themePromise;
}

export function getIntegrationReport(): Promise<IntegrationReport> {
  return invoke<IntegrationReport>("get_integration_report").then((report) => {
    integrationReportCache = report;
    return report;
  });
}

export function runRepairAction(actionId: string): Promise<RepairRunResult> {
  return invoke<RepairRunResult>("run_repair_action", { actionId }).then((result) => {
    integrationReportCache = result.report;
    dashboardSnapshotCache = result.report.snapshot;
    logsCache = null;
    return result;
  });
}

export function captureCurrentAccount(agent: "codex" | "claude"): Promise<AccountReport> {
  return invoke<AccountReport>("capture_current_account", { agent }).then(cacheAccountReport);
}

export function switchAccountSlot(agent: "codex" | "claude", label: string): Promise<AccountReport> {
  return invoke<AccountReport>("switch_account_slot", { agent, label }).then(cacheAccountReport);
}

export function getLimitAlerts(): Promise<LimitAlert[]> {
  return invoke<LimitAlert[]>("get_limit_alerts");
}

export function dismissLimitAlert(agent: "codex" | "claude"): Promise<void> {
  return invoke<void>("dismiss_limit_alert", { agent });
}

export function deleteAccountSlot(agent: "codex" | "claude", label: string): Promise<AccountReport> {
  return invoke<AccountReport>("delete_account_slot", { agent, label }).then(cacheAccountReport);
}

export function getConfigSettings(options: { force?: boolean } = {}): Promise<ConfigRow[]> {
  if (!options.force && configRowsCache) return Promise.resolve(configRowsCache);
  if (!options.force && configRowsPromise) return configRowsPromise;
  configRowsPromise = invoke<ConfigRow[]>("get_config_settings")
    .then((rows) => {
      configRowsCache = rows;
      return rows;
    })
    .finally(() => {
      configRowsPromise = null;
    });
  return configRowsPromise;
}

export function setConfigValue(key: string, value: string): Promise<ConfigRow[]> {
  return invoke<ConfigRow[]>("set_config_value", { key, value }).then((rows) => {
    configRowsCache = rows;
    if (key.startsWith("theme.") || key.startsWith("gui_theme.")) invalidateDashboardCache();
    return rows;
  });
}

export function resetConfigValue(key: string): Promise<ConfigRow[]> {
  return invoke<ConfigRow[]>("reset_config_value", { key }).then((rows) => {
    configRowsCache = rows;
    if (key.startsWith("theme.") || key.startsWith("gui_theme.")) invalidateDashboardCache();
    return rows;
  });
}

export function toggleCapsuleState(path: string): Promise<string> {
  return invoke<string>("toggle_capsule_state", { path }).then((state) => {
    dashboardSnapshotCache = null;
    return state;
  });
}

export function setCapsuleState(path: string, state: string): Promise<string> {
  return invoke<string>("set_capsule_state", { path, state }).then((nextState) => {
    dashboardSnapshotCache = null;
    return nextState;
  });
}

export function setCapsuleField(path: string, field: string, value: string): Promise<void> {
  return invoke<void>("set_capsule_field", { path, field, value }).then(() => {
    dashboardSnapshotCache = null;
  });
}

export function deleteCapsule(path: string): Promise<void> {
  return invoke<void>("delete_capsule", { path }).then(() => {
    dashboardSnapshotCache = null;
  });
}

export function openCapsuleFolder(path: string): Promise<MenuCommandResult> {
  return invoke("open_capsule_folder", { path });
}

export function openCapsuleExternal(path: string): Promise<MenuCommandResult> {
  return invoke("open_capsule_external", { path });
}

export function runDoctor(): Promise<IntegrationReport> {
  return invoke<IntegrationReport>("run_doctor").then((report) => {
    integrationReportCache = report;
    dashboardSnapshotCache = report.snapshot;
    return report;
  });
}

export function getCachedIntegrationReport(): IntegrationReport | null {
  return integrationReportCache;
}

export function createCheckpoint(): Promise<MenuCommandResult> {
  return invoke<MenuCommandResult>("create_checkpoint").then((result) => {
    dashboardSnapshotCache = null;
    return result;
  });
}

export function openLogsFolder(): Promise<MenuCommandResult> {
  return invoke("open_logs_folder");
}

export function openAccountsFolder(): Promise<MenuCommandResult> {
  return invoke("open_accounts_folder");
}

export function reinstallHooks(): Promise<MenuCommandResult> {
  return invoke<MenuCommandResult>("reinstall_hooks").then((result) => {
    dashboardSnapshotCache = null;
    integrationReportCache = null;
    logsCache = null;
    return result;
  });
}

export function ensureDaemonRunning(): Promise<MenuCommandResult> {
  return invoke<MenuCommandResult>("ensure_daemon_running").then((result) => {
    dashboardSnapshotCache = null;
    integrationReportCache = null;
    return result;
  });
}

export function openProjectGithub(): Promise<MenuCommandResult> {
  return invoke("open_project_github");
}

export function getAppVersion(): Promise<string> {
  return invoke("get_app_version");
}

export function checkAppUpdate(): Promise<UpdateStatus> {
  return invoke("check_app_update");
}

export function runAppUpdate(): Promise<MenuCommandResult> {
  return invoke("run_app_update");
}

export function runAppUninstall(): Promise<MenuCommandResult> {
  return invoke("run_app_uninstall");
}
