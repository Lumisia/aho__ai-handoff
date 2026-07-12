import {
  Bot,
  Cable,
  ChartColumnIncreasing,
  ChevronDown,
  ChevronRight,
  CircleUserRound,
  Copy,
  ExternalLink,
  FolderKanban,
  FolderOpen,
  Github,
  LayoutDashboard,
  Minus,
  PanelLeftClose,
  PanelLeftOpen,
  RefreshCw,
  Search,
  Settings,
  Square,
  Trash2,
  X,
} from "lucide-react";
import ClaudeIcon from "@lobehub/icons/es/Claude";
import OpenAIIcon from "@lobehub/icons/es/OpenAI";
import * as Menubar from "@radix-ui/react-menubar";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  checkAppUpdate,
  createCheckpoint,
  deleteCapsule,
  dismissLimitAlert,
  ensureDaemonRunning,
  getAccountReport,
  getAppVersion,
  getDashboardSnapshot,
  getLimitAlerts,
  getTheme,
  openCapsuleExternal,
  openCapsuleFolder,
  openLogsFolder,
  openProjectGithub,
  pollAccountLogin,
  refreshAccountSlotUsage,
  reinstallHooks,
  runAppUpdate,
  runDoctor,
  startAccountLogin,
  switchAccountSlot,
} from "./api";
import { createTranslator, normalizeLanguage } from "./i18n";
import { LimitBar, ResetCreditsBlock } from "./components/AccountUsage";
import type { CSSProperties, MouseEvent, PointerEvent as ReactPointerEvent, ReactNode } from "react";
import type {
  AccountLoginSession,
  AccountSlotRow,
  CapsuleSummary,
  DashboardSnapshot,
  LimitAlert,
  SlotUsageReport,
  ThemeReport,
  UpdateStatus,
} from "./types";
import type { Translator } from "./i18n";
import Account from "./views/Account";
import Capsules, { capsuleStates, stateLabel } from "./views/Capsules";
import Integration from "./views/Integration";
import Overview from "./views/Overview";
import SettingsView from "./views/Settings";
import Usage from "./views/Usage";

type Tab = "overview" | "capsules" | "usage" | "account" | "integration" | "settings";
// Known agents get branded icons; any other agent id renders generically.
type AgentName = string;

interface CapsuleProjectNode {
  project_id: string;
  project_label: string;
  capsules: CapsuleSummary[];
}

interface CapsuleAgentNode {
  agent: AgentName;
  projects: CapsuleProjectNode[];
  count: number;
}

interface CapsuleContextMenuState {
  x: number;
  y: number;
  item: CapsuleSummary;
}

const navTabs: Array<{ id: Exclude<Tab, "capsules" | "settings">; labelKey: string; icon: typeof LayoutDashboard }> = [
  { id: "overview", labelKey: "overview", icon: LayoutDashboard },
  { id: "usage", labelKey: "usage", icon: ChartColumnIncreasing },
  { id: "account", labelKey: "account", icon: CircleUserRound },
  { id: "integration", labelKey: "integration", icon: Cable },
];

function displayAgent(source: string): AgentName {
  const lower = source.toLowerCase();
  if (lower.includes("claude")) return "Claude";
  if (lower.includes("codex")) return "Codex";
  // Unknown agents (grok, gemini, cursor, ...) keep their own bucket.
  return source.charAt(0).toUpperCase() + source.slice(1);
}

function targetAgent(target: string) {
  const lower = target.toLowerCase();
  if (lower.includes("claude")) return "Claude";
  if (lower.includes("codex")) return "Codex";
  // Open capsules ("any") and future agents (grok, gemini, ...) show as-is.
  return target;
}

function hexLuminance(value?: string | null) {
  if (!value) return null;
  const match = value.trim().match(/^#?([0-9a-fA-F]{6})$/);
  if (!match) return null;
  const channels = [0, 2, 4].map((offset) => {
    const raw = parseInt(match[1].slice(offset, offset + 2), 16) / 255;
    return raw <= 0.03928 ? raw / 12.92 : ((raw + 0.055) / 1.055) ** 2.4;
  });
  return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
}

interface ResolvedThemeColors {
  focus_border_color: string;
  selection_bg_color: string;
  selection_fg_color: string;
  app_bg_color: string;
  sidebar_bg_color: string;
  panel_bg_color: string;
  text_color: string;
  codex_color: string;
  claude_color: string;
}

function isDarkColors(colors: ResolvedThemeColors) {
  const surface = hexLuminance(colors.app_bg_color) ?? hexLuminance(colors.panel_bg_color);
  return surface !== null && surface < 0.35;
}

/// Resolve which catalog theme is active given the mode and the OS preference,
/// then hand back its colors. "custom" uses the config's own color fields.
function resolveTheme(
  theme: ThemeReport | null,
  osDark: boolean,
): { colors: ResolvedThemeColors; dark: boolean } | null {
  if (!theme) return null;
  const custom: ResolvedThemeColors = {
    focus_border_color: theme.focus_border_color,
    selection_bg_color: theme.selection_bg_color,
    selection_fg_color: theme.selection_fg_color,
    app_bg_color: theme.app_bg_color,
    sidebar_bg_color: theme.sidebar_bg_color,
    panel_bg_color: theme.panel_bg_color,
    text_color: theme.text_color,
    codex_color: theme.codex_color,
    claude_color: theme.claude_color,
  };
  const activeId =
    theme.mode === "light"
      ? theme.light_theme
      : theme.mode === "dark"
        ? theme.dark_theme
        : osDark
          ? theme.dark_theme
          : theme.light_theme;
  if (activeId === "custom") {
    return { colors: custom, dark: isDarkColors(custom) };
  }
  const entry = theme.catalog.find((item) => item.id === activeId);
  if (!entry) return { colors: custom, dark: isDarkColors(custom) };
  return {
    colors: {
      focus_border_color: entry.focus_border_color,
      selection_bg_color: entry.selection_bg_color,
      selection_fg_color: entry.selection_fg_color,
      app_bg_color: entry.app_bg_color,
      sidebar_bg_color: entry.sidebar_bg_color,
      panel_bg_color: entry.panel_bg_color,
      text_color: entry.text_color,
      codex_color: entry.codex_color,
      claude_color: entry.claude_color,
    },
    dark: entry.dark,
  };
}

function shortDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value.slice(0, 10);
  return date.toLocaleDateString(undefined, { month: "2-digit", day: "2-digit" });
}

function buildCapsuleTree(items: CapsuleSummary[]): CapsuleAgentNode[] {
  const tree: CapsuleAgentNode[] = [
    { agent: "Codex", projects: [], count: 0 },
    { agent: "Claude", projects: [], count: 0 },
  ];
  for (const item of items) {
    const name = displayAgent(item.source_agent);
    let agent = tree.find((node) => node.agent === name);
    if (!agent) {
      agent = { agent: name, projects: [], count: 0 };
      tree.push(agent);
    }
    agent.count += 1;
    let project = agent.projects.find((node) => node.project_id === item.project_id);
    if (!project) {
      project = {
        project_id: item.project_id,
        project_label: item.project_label,
        capsules: [],
      };
      agent.projects.push(project);
    }
    project.capsules.push(item);
  }
  return tree;
}

function AgentLogo({ agent }: { agent: AgentName }) {
  if (agent === "Codex") {
    return (
      <span className="agent-logo codex" aria-hidden="true">
        <OpenAIIcon size={16} />
      </span>
    );
  }
  if (agent === "Claude") {
    return (
      <span className="agent-logo claude" aria-hidden="true">
        <ClaudeIcon size={16} />
      </span>
    );
  }
  return (
    <span className="agent-logo other" aria-hidden="true">
      <Bot size={16} />
    </span>
  );
}

function MenuItem({
  children,
  onSelect,
}: {
  children: ReactNode;
  onSelect: () => void;
}) {
  return (
    <Menubar.Item
      className="menu-item"
      onSelect={(event) => {
        event.preventDefault();
        onSelect();
      }}
    >
      {children}
    </Menubar.Item>
  );
}

function AppMenubar({
  t,
  onCheckpoint,
  onOpenLogs,
  onAddAccount,
  onAccountStatus,
  onDoctor,
  onReinstallHooks,
  onRestartDaemon,
  onOpenGithub,
}: {
  t: Translator;
  onCheckpoint: () => void;
  onOpenLogs: () => void;
  onAddAccount: (agent: "codex" | "claude") => void;
  onAccountStatus: () => void;
  onDoctor: () => void;
  onReinstallHooks: () => void;
  onRestartDaemon: () => void;
  onOpenGithub: () => void;
}) {
  return (
    <Menubar.Root className="app-menubar">
      <Menubar.Menu>
        <Menubar.Trigger className="menu-trigger">{t("fileMenu")}</Menubar.Trigger>
        <Menubar.Portal>
          <Menubar.Content className="menu-content" align="start">
            <MenuItem onSelect={onCheckpoint}>{t("newCheckpoint")}</MenuItem>
            <MenuItem onSelect={onOpenLogs}>{t("openLogsFolder")}</MenuItem>
          </Menubar.Content>
        </Menubar.Portal>
      </Menubar.Menu>

      <Menubar.Menu>
        <Menubar.Trigger className="menu-trigger">{t("accountMenu")}</Menubar.Trigger>
        <Menubar.Portal>
          <Menubar.Content className="menu-content" align="start">
            <MenuItem onSelect={() => onAddAccount("claude")}>{t("addClaudeAccount")}</MenuItem>
            <MenuItem onSelect={() => onAddAccount("codex")}>{t("addCodexAccount")}</MenuItem>
            <Menubar.Separator className="menu-separator" />
            <MenuItem onSelect={onAccountStatus}>{t("accountStatus")}</MenuItem>
          </Menubar.Content>
        </Menubar.Portal>
      </Menubar.Menu>

      <Menubar.Menu>
        <Menubar.Trigger className="menu-trigger">{t("toolsMenu")}</Menubar.Trigger>
        <Menubar.Portal>
          <Menubar.Content className="menu-content" align="start">
            <MenuItem onSelect={onDoctor}>{t("runDoctor")}</MenuItem>
            <MenuItem onSelect={onReinstallHooks}>{t("reinstallHooks")}</MenuItem>
            <MenuItem onSelect={onRestartDaemon}>{t("restartDaemon")}</MenuItem>
          </Menubar.Content>
        </Menubar.Portal>
      </Menubar.Menu>

      <Menubar.Menu>
        <Menubar.Trigger className="menu-trigger">{t("helpMenu")}</Menubar.Trigger>
        <Menubar.Portal>
          <Menubar.Content className="menu-content" align="start">
            <MenuItem onSelect={onOpenGithub}>
              <Github size={14} />
              <span>{t("openGithub")}</span>
            </MenuItem>
          </Menubar.Content>
        </Menubar.Portal>
      </Menubar.Menu>
    </Menubar.Root>
  );
}

function WindowControls({ t }: { t: Translator }) {
  const appWindow = getCurrentWindow();
  return (
    <div className="window-controls" aria-label="Window controls">
      <button className="window-control" title={t("minimize")} onClick={() => void appWindow.minimize()}>
        <Minus size={14} />
      </button>
      <button className="window-control" title={t("maximize")} onClick={() => void appWindow.toggleMaximize()}>
        <Square size={12} />
      </button>
      <button className="window-control close" title={t("close")} onClick={() => void appWindow.close()}>
        <X size={15} />
      </button>
    </div>
  );
}

function fmtTemplate(template: string, vars: Record<string, string>) {
  return template.replace(/\{(\w+)\}/g, (_, key) => vars[key] ?? `{${key}}`);
}

function LimitSwitchAccountCard({
  agent,
  slot,
  usage,
  busy,
  t,
  onActivate,
  current,
}: {
  agent: "codex" | "claude";
  slot: AccountSlotRow;
  usage?: SlotUsageReport | null;
  busy?: boolean;
  t: Translator;
  onActivate?: () => void;
  current?: boolean;
}) {
  const plan = usage?.plan ?? slot.plan;
  return (
    <article className={`limit-switch-account${current ? " current" : ""}`}>
      <div className="limit-switch-account-head">
        <div className="limit-switch-account-id">
          <strong>{slot.email || slot.label}</strong>
          {plan && <small>{plan}</small>}
        </div>
        {onActivate && (
          <button className="ghost" onClick={onActivate} title={t("limitSwitchPick")}>
            {t("switch")}
          </button>
        )}
      </div>
      {busy ? (
        <div className="limit-switch-account-loading">{t("loadingAccounts")}</div>
      ) : (
        <div className="limit-switch-bars">
          <LimitBar agent={agent} label="5h" value={usage?.five_hour} t={t} compact />
          <LimitBar agent={agent} label={t("weekly")} value={usage?.weekly} t={t} compact />
          <ResetCreditsBlock usage={usage} t={t} />
        </div>
      )}
    </article>
  );
}

function LimitSwitchModal({
  alert,
  t,
  onSwitch,
  onDismiss,
}: {
  alert: LimitAlert | null;
  t: Translator;
  onSwitch: (alert: LimitAlert, label: string) => void;
  onDismiss: (alert: LimitAlert) => void;
}) {
  const [usage, setUsage] = useState<Record<string, SlotUsageReport>>({});
  const [busy, setBusy] = useState<Set<string>>(new Set());
  const agent = alert?.agent;
  // Re-run the lazy fetch whenever the popup opens for a different alert.
  const slotKey = alert ? alert.slots.map((slot) => slot.label).join(",") : "";

  useEffect(() => {
    if (!alert) return;
    let cancelled = false;
    // Fetch each candidate slot's real provider usage when the popup opens —
    // same explicit per-slot path the Account tab uses.
    setUsage({});
    setBusy(new Set(alert.slots.map((slot) => slot.label)));
    for (const slot of alert.slots) {
      refreshAccountSlotUsage(alert.agent, slot.label)
        .then((result) => {
          if (!cancelled) setUsage((current) => ({ ...current, [slot.label]: result }));
        })
        .catch(() => {
          /* leave this slot without a sample; the card shows "no sample" */
        })
        .finally(() => {
          if (!cancelled) {
            setBusy((current) => {
              const next = new Set(current);
              next.delete(slot.label);
              return next;
            });
          }
        });
    }
    return () => {
      cancelled = true;
    };
  }, [agent, slotKey]);

  if (!alert) return null;
  const agentName = alert.agent === "claude" ? "Claude" : "Codex";
  const body = fmtTemplate(t("limitSwitchBody"), {
    agent: agentName,
    used: String(Math.round(alert.used_percent)),
    threshold: String(Math.round(alert.threshold_percent)),
  });
  return (
    <div className="modal-backdrop" onMouseDown={() => onDismiss(alert)}>
      <section
        className="limit-switch-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="limit-switch-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal-header">
          <h2 id="limit-switch-title">
            <AgentLogo agent={agentName} /> {t("limitSwitchTitle")}
          </h2>
        </header>
        <p className="limit-switch-body">{body}</p>
        {alert.agent_running && (
          <p className="limit-switch-warning">
            {fmtTemplate(t("limitSwitchRunningWarning"), { agent: agentName })}
          </p>
        )}

        <div className="limit-switch-columns">
          <div className="limit-switch-col limit-switch-col-current">
            <div className="limit-switch-section-label">{t("limitSwitchCurrent")}</div>
            <LimitSwitchAccountCard
              agent={alert.agent}
              slot={alert.active_slot}
              usage={alert.active_usage}
              t={t}
              current
            />
          </div>
          <div className="limit-switch-col limit-switch-col-others">
            <div className="limit-switch-section-label">{t("limitSwitchOthers")}</div>
            <div className="limit-switch-others-list">
              {alert.slots.map((slot) => (
                <LimitSwitchAccountCard
                  key={slot.path}
                  agent={alert.agent}
                  slot={slot}
                  usage={usage[slot.label]}
                  busy={busy.has(slot.label)}
                  t={t}
                  onActivate={() => onSwitch(alert, slot.label)}
                />
              ))}
            </div>
          </div>
        </div>

        <footer className="limit-switch-actions">
          <button className="limit-switch-later" onClick={() => onDismiss(alert)}>
            {t("limitSwitchLater")}
          </button>
        </footer>
      </section>
    </div>
  );
}

function SettingsModal({
  open,
  snapshot,
  onClose,
  onThemeChanged,
  t,
}: {
  open: boolean;
  snapshot: DashboardSnapshot | null;
  onClose: () => void;
  onThemeChanged: () => Promise<void> | void;
  t: Translator;
}) {
  if (!open || !snapshot) return null;
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <section
        className="settings-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-modal-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal-header">
          <h2 id="settings-modal-title">{t("settings")}</h2>
          <button className="titlebar-icon-button" title={t("close")} onClick={onClose}>
            <X size={16} />
          </button>
        </header>
        <SettingsView onThemeChanged={onThemeChanged} t={t} />
      </section>
    </div>
  );
}

function CapsuleContextMenu({
  state,
  t,
  onClose,
  onDelete,
  onCopyPath,
  onOpenFolder,
  onOpenWith,
}: {
  state: CapsuleContextMenuState | null;
  t: Translator;
  onClose: () => void;
  onDelete: (item: CapsuleSummary) => void;
  onCopyPath: (item: CapsuleSummary) => void;
  onOpenFolder: (item: CapsuleSummary) => void;
  onOpenWith: (item: CapsuleSummary) => void;
}) {
  if (!state) return null;
  const actions = [
    { label: t("deleteCapsule"), icon: Trash2, run: onDelete, danger: true },
    { label: t("copyPath"), icon: Copy, run: onCopyPath },
    { label: t("openFolder"), icon: FolderOpen, run: onOpenFolder },
    { label: t("openWith"), icon: ExternalLink, run: onOpenWith },
  ];
  return (
    <div className="context-scrim" onMouseDown={onClose}>
      <section
        className="capsule-context-menu"
        style={{ left: state.x, top: state.y }}
        onMouseDown={(event) => event.stopPropagation()}
      >
        {actions.map((action) => {
          const Icon = action.icon;
          return (
            <button
              key={action.label}
              className={action.danger ? "context-item danger-item" : "context-item"}
              onClick={() => {
                action.run(state.item);
                onClose();
              }}
            >
              <Icon size={15} />
              <span>{action.label}</span>
            </button>
          );
        })}
      </section>
    </div>
  );
}

function OverviewHeaderStats({ snapshot, t }: { snapshot: DashboardSnapshot; t: Translator }) {
  const items = [
    { label: t("pending"), value: snapshot.capsules.pending_count },
    { label: t("capsules"), value: snapshot.capsules.items.length },
    { label: t("skippedFiles"), value: snapshot.capsules.skipped },
  ];

  return (
    <div className="overview-header-stats" aria-label={t("overview")}>
      {items.map((item) => (
        <span key={item.label}>
          <span>{item.label}</span>
          <strong>{item.value}</strong>
        </span>
      ))}
    </div>
  );
}

function AppTitlebar({
  t,
  snapshot,
  sidebarCollapsed,
  refreshing,
  statusMessage,
  onToggleSidebar,
  onRefresh,
  onSettings,
  onCheckpoint,
  onOpenLogs,
  onAddAccount,
  onAccountStatus,
  onDoctor,
  onReinstallHooks,
  onRestartDaemon,
  onOpenGithub,
}: {
  t: Translator;
  snapshot: DashboardSnapshot | null;
  sidebarCollapsed: boolean;
  refreshing: boolean;
  statusMessage: string | null;
  onToggleSidebar: () => void;
  onRefresh: () => void;
  onSettings: () => void;
  onCheckpoint: () => void;
  onOpenLogs: () => void;
  onAddAccount: (agent: "codex" | "claude") => void;
  onAccountStatus: () => void;
  onDoctor: () => void;
  onReinstallHooks: () => void;
  onRestartDaemon: () => void;
  onOpenGithub: () => void;
}) {
  const pending = snapshot?.capsules.pending_count ?? 0;
  const attentionCount =
    snapshot?.checks.filter((check) => check.status === "warning" || check.status === "error" || check.status === "missing")
      .length ?? 0;
  const context = snapshot
    ? `${t("pending")} ${pending}${attentionCount > 0 ? ` - ${t("attention")} ${attentionCount}` : ""}`
    : t("titlebarContextEmpty");
  const status = statusMessage ?? context;
  const SidebarIcon = sidebarCollapsed ? PanelLeftOpen : PanelLeftClose;

  return (
    <header className="app-titlebar">
      <div className="titlebar-left">
        <img className="titlebar-app-icon" src="/aho.ico" alt="" aria-hidden="true" />
        <button className="titlebar-icon-button" title={t("toggleSidebar")} onClick={onToggleSidebar}>
          <SidebarIcon size={16} />
        </button>
        <AppMenubar
          t={t}
          onCheckpoint={onCheckpoint}
          onOpenLogs={onOpenLogs}
          onAddAccount={onAddAccount}
          onAccountStatus={onAccountStatus}
          onDoctor={onDoctor}
          onReinstallHooks={onReinstallHooks}
          onRestartDaemon={onRestartDaemon}
          onOpenGithub={onOpenGithub}
        />
      </div>

      <div className="titlebar-drag" data-tauri-drag-region onDoubleClick={() => void getCurrentWindow().toggleMaximize()}>
        <span className={`health-dot ${attentionCount === 0 ? "ok" : "warn"}`} />
        <span>{status}</span>
      </div>

      <div className="titlebar-right">
        <button className="titlebar-icon-button" title={t("refresh")} onClick={onRefresh} disabled={refreshing}>
          <RefreshCw className={refreshing ? "spin" : undefined} size={16} />
        </button>
        <button className="titlebar-icon-button" title={t("settings")} onClick={onSettings}>
          <Settings size={16} />
        </button>
        <span className="titlebar-gap" />
        <WindowControls t={t} />
      </div>
    </header>
  );
}

/// Client-side capsule filter: the list is already in memory, so search stays
/// instant and never touches the backend.
function filterCapsules(items: CapsuleSummary[], query: string, state: string) {
  const needle = query.trim().toLowerCase();
  return items.filter((item) => {
    if (state !== "all" && item.state !== state) return false;
    if (!needle) return true;
    return [
      item.summary_preview,
      item.capsule_id,
      item.project_label,
      item.source_agent,
      item.target_agent,
    ].some((field) => field.toLowerCase().includes(needle));
  });
}

function CapsuleNavigator({
  items,
  selectedPath,
  openAgents,
  openProjectKeys,
  query,
  stateFilter,
  onQueryChange,
  onStateFilterChange,
  onToggleAgent,
  onToggleProject,
  onSelectCapsule,
  onCapsuleContextMenu,
  t,
}: {
  items: CapsuleSummary[];
  selectedPath?: string | null;
  openAgents: AgentName[];
  openProjectKeys: string[];
  query: string;
  stateFilter: string;
  onQueryChange: (query: string) => void;
  onStateFilterChange: (state: string) => void;
  onToggleAgent: (agent: AgentName) => void;
  onToggleProject: (key: string) => void;
  onSelectCapsule: (path: string) => void;
  onCapsuleContextMenu: (event: MouseEvent, item: CapsuleSummary) => void;
  t: Translator;
}) {
  const filterActive = query.trim() !== "" || stateFilter !== "all";
  const visible = useMemo(
    () => filterCapsules(items, query, stateFilter),
    [items, query, stateFilter],
  );
  const tree = useMemo(() => buildCapsuleTree(visible), [visible]);
  // While a filter is active the matches are the point — the tree opens fully
  // instead of making the user re-expand branches to see them.
  const agentOpen = (agent: AgentName) => filterActive || openAgents.includes(agent);
  const projectOpen = (key: string) => filterActive || openProjectKeys.includes(key);

  return (
    <section className="sidebar-capsules" aria-label={t("capsules")}>
      <div className="sidebar-section-title">
        <FolderKanban size={15} />
        <span>{t("capsules")}</span>
      </div>
      <div className="sidebar-capsule-filter">
        <input
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder={t("capsuleSearch")}
          aria-label={t("capsuleSearch")}
        />
        <select
          value={stateFilter}
          onChange={(event) => onStateFilterChange(event.target.value)}
          aria-label={t("statusLabel")}
        >
          <option value="all">{t("allStates")}</option>
          {capsuleStates.map((state) => (
            <option key={state} value={state}>
              {stateLabel(t, state)}
            </option>
          ))}
        </select>
      </div>
      {items.length === 0 && <div className="sidebar-empty">{t("noCapsules")}</div>}
      {items.length > 0 && filterActive && visible.length === 0 && (
        <div className="sidebar-empty">{t("noMatches")}</div>
      )}
      {tree.filter((agent) => !filterActive || agent.count > 0).map((agent) => (
        <div className={`side-agent ${agent.agent.toLowerCase()}`} key={agent.agent}>
          <button className="side-agent-row" onClick={() => onToggleAgent(agent.agent)}>
            <span className="agent-title">
              {agentOpen(agent.agent) ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
              <AgentLogo agent={agent.agent} />
              <strong>{agent.agent === "Codex" ? t("codex") : t("claude")}</strong>
            </span>
            <span className="side-count">{agent.count}</span>
          </button>
          {agentOpen(agent.agent) && agent.projects.map((project) => {
            const key = `${agent.agent}-${project.project_id}`;
            const open = projectOpen(key);
            return (
              <div className="side-project" key={key}>
                <button
                  className={open ? "side-project-button open" : "side-project-button"}
                  onClick={() => onToggleProject(key)}
                  title={project.project_id}
                >
                  <span className="project-title">
                    {open ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
                    <span>{project.project_label}</span>
                  </span>
                  <small>{project.capsules.length}</small>
                </button>
                {open && (
                  <div className="side-capsule-list">
                    {project.capsules.map((item) => (
                      <button
                        className={selectedPath === item.path ? "side-capsule active" : "side-capsule"}
                        key={item.path}
                        onClick={() => onSelectCapsule(item.path)}
                        onContextMenu={(event) => onCapsuleContextMenu(event, item)}
                      >
                        <span>{shortDate(item.created_at)}</span>
                        <strong>{item.summary_preview || item.capsule_id}</strong>
                        <small>
                          {item.state} / {targetAgent(item.target_agent)}
                        </small>
                      </button>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      ))}
    </section>
  );
}

export default function App() {
  const [active, setActive] = useState<Tab>("overview");
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [theme, setTheme] = useState<ThemeReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [selectedCapsulePath, setSelectedCapsulePath] = useState<string | null>(null);
  const [openAgents, setOpenAgents] = useState<AgentName[]>(["Codex", "Claude"]);
  const [openProjectKeys, setOpenProjectKeys] = useState<string[]>([]);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [sidebarWidth, setSidebarWidth] = useState(292);
  const [sidebarResizing, setSidebarResizing] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [capsuleMenu, setCapsuleMenu] = useState<CapsuleContextMenuState | null>(null);
  const [titleStatus, setTitleStatus] = useState<string | null>(null);
  const [titlebarLogin, setTitlebarLogin] = useState<AccountLoginSession | null>(null);
  const [limitAlert, setLimitAlert] = useState<LimitAlert | null>(null);
  const [capsuleQuery, setCapsuleQuery] = useState("");
  const [capsuleStateFilter, setCapsuleStateFilter] = useState("all");
  const [appVersion, setAppVersion] = useState("");
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
  const [osDark, setOsDark] = useState(
    () => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false,
  );
  const refreshSeq = useRef(0);
  const seededProjectRef = useRef(false);
  const sidebarDragOffset = useRef(0);
  const suppressedLimitAlerts = useRef(new Set<string>());
  const language = normalizeLanguage(theme?.language);
  const t = useMemo(() => createTranslator(language), [language]);

  function reportMenuError(err: unknown) {
    setTitleStatus(`${t("menuActionFailed")}: ${err instanceof Error ? err.message : String(err)}`);
  }

  function reportMenuMessage(message: string) {
    setTitleStatus(message);
  }

  async function refresh(options: { force?: boolean } = {}) {
    if (refreshing) return;
    const seq = refreshSeq.current + 1;
    refreshSeq.current = seq;
    setRefreshing(true);
    try {
      setError(null);
      const [nextSnapshot, nextTheme] = await Promise.all([
        getDashboardSnapshot(options),
        getTheme(options),
      ]);
      if (seq !== refreshSeq.current) return;
      setSnapshot(nextSnapshot);
      setTheme(nextTheme);
      if (!selectedCapsulePath && nextSnapshot.capsules.items[0]) {
        setSelectedCapsulePath(nextSnapshot.capsules.items[0].path);
      }
    } catch (err) {
      if (seq !== refreshSeq.current) return;
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (seq === refreshSeq.current) setRefreshing(false);
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  // One-shot startup lookups: version chip and the daily update check
  // (cached in the backend, opt-out via gui.update_check).
  useEffect(() => {
    getAppVersion()
      .then(setAppVersion)
      .catch(() => {});
    checkAppUpdate()
      .then(setUpdateStatus)
      .catch(() => {});
  }, []);

  // Track the OS light/dark preference so "system" mode auto-switches themes.
  useEffect(() => {
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!mq) return;
    const handler = (event: MediaQueryListEvent) => setOsDark(event.matches);
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);

  useEffect(() => {
    if (!titlebarLogin) return;
    const timer = window.setInterval(() => {
      pollAccountLogin(titlebarLogin.agent, titlebarLogin.home)
        .then((poll) => {
          setTitleStatus(poll.message);
          if (poll.done) {
            setTitlebarLogin(null);
            void refresh({ force: true });
          }
        })
        .catch((err) => {
          setTitlebarLogin(null);
          reportMenuError(err);
        });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [titlebarLogin]);

  // Poll for limit-reached account-switch popups. Only shows one at a time; a
  // dismiss/switch marks the reset window so it does not immediately reappear.
  useEffect(() => {
    let cancelled = false;
    function poll() {
      if (limitAlert) return;
      getLimitAlerts()
        .then((alerts) => {
          const next = alerts.find((alert) => !suppressedLimitAlerts.current.has(limitAlertKey(alert)));
          if (!cancelled && next && !limitAlert) {
            setLimitAlert(next);
          }
        })
        .catch(() => {
          /* best-effort: a failed poll must never disrupt the app */
        });
    }
    poll();
    // 5-minute cadence: each poll may do a live provider-usage fetch for the
    // active account, and five-hour limits move slowly, so 60s would be wasteful.
    const timer = window.setInterval(poll, 300_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [limitAlert]);

  function limitAlertKey(alert: LimitAlert) {
    return `${alert.agent}:${alert.resets_at ?? "unknown"}`;
  }

  function handleLimitDismiss(alert: LimitAlert) {
    const key = limitAlertKey(alert);
    suppressedLimitAlerts.current.add(key);
    setLimitAlert(null);
    void dismissLimitAlert(alert.agent)
      .then(() => {
        suppressedLimitAlerts.current.delete(key);
      })
      .catch(() => {
        /* keep this alert quiet for the current app session */
      });
  }

  function handleLimitSwitch(alert: LimitAlert, label: string) {
    const key = limitAlertKey(alert);
    suppressedLimitAlerts.current.add(key);
    setActive("account");
    setLimitAlert(null);
    void runTitlebarAction(async () => {
      try {
        await switchAccountSlot(alert.agent, label);
        return { message: fmtTemplate(t("limitSwitchDone"), { label }) };
      } finally {
        // Either way, quiet the popup for this reset window.
        try {
          await dismissLimitAlert(alert.agent);
          suppressedLimitAlerts.current.delete(key);
        } catch {
          /* keep this alert quiet for the current app session */
        }
        await refresh({ force: true });
      }
    });
  }

  useEffect(() => {
    if (!snapshot || seededProjectRef.current) return;
    const first = buildCapsuleTree(snapshot.capsules.items)
      .flatMap((agent) => agent.projects.map((project) => `${agent.agent}-${project.project_id}`))[0];
    if (!first) return;
    setOpenProjectKeys([first]);
    seededProjectRef.current = true;
  }, [snapshot]);

  useEffect(() => {
    if (!settingsOpen && !capsuleMenu) return;
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setSettingsOpen(false);
        setCapsuleMenu(null);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [settingsOpen, capsuleMenu]);

  useEffect(() => {
    if (!sidebarResizing) return;
    function onPointerMove(event: PointerEvent) {
      const nextWidth = event.clientX - sidebarDragOffset.current;
      setSidebarWidth(Math.min(420, Math.max(220, nextWidth)));
    }
    function onPointerUp() {
      setSidebarResizing(false);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp, { once: true });
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
    };
  }, [sidebarResizing]);

  function startSidebarResize(event: ReactPointerEvent<HTMLButtonElement>) {
    event.preventDefault();
    sidebarDragOffset.current = event.clientX - sidebarWidth;
    setSidebarResizing(true);
  }

  function toggleAgent(agent: AgentName) {
    setOpenAgents((current) =>
      current.includes(agent) ? current.filter((item) => item !== agent) : [...current, agent],
    );
  }

  function toggleProject(key: string) {
    setOpenProjectKeys((current) =>
      current.includes(key) ? current.filter((item) => item !== key) : [...current, key],
    );
  }

  async function runTitlebarAction(action: () => Promise<{ message?: string } | void>) {
    try {
      const result = await action();
      if (result?.message) reportMenuMessage(result.message);
    } catch (err) {
      reportMenuError(err);
    }
  }

  function handleCheckpoint() {
    void runTitlebarAction(async () => createCheckpoint());
  }

  function handleOpenLogs() {
    void runTitlebarAction(async () => openLogsFolder());
  }

  function handleAddAccount(agent: "codex" | "claude") {
    setActive("account");
    void runTitlebarAction(async () => {
      const session = await startAccountLogin(agent);
      setTitlebarLogin(session);
      return session;
    });
  }

  function handleAccountStatus() {
    setActive("account");
    void runTitlebarAction(async () => {
      await getAccountReport({ force: true });
      return { message: t("accountStatus") };
    });
  }

  function handleDoctor() {
    setActive("integration");
    void runTitlebarAction(async () => {
      const report = await runDoctor();
      return { message: `doctor: ${report.doctor.ok}/${report.doctor.warn}/${report.doctor.fail}` };
    });
  }

  function handleReinstallHooks() {
    if (!window.confirm(t("confirmReinstallHooks"))) return;
    void runTitlebarAction(async () => {
      const result = await reinstallHooks();
      void refresh({ force: true });
      return result;
    });
  }

  function handleRestartDaemon() {
    void runTitlebarAction(async () => {
      const result = await ensureDaemonRunning();
      void refresh({ force: true });
      return result;
    });
  }

  function handleOpenGithub() {
    void runTitlebarAction(async () => openProjectGithub());
  }

  function handleUpdateNow() {
    const latest = updateStatus?.latest;
    if (!latest) return;
    if (!window.confirm(fmtTemplate(t("updateConfirm"), { version: latest }))) return;
    void runTitlebarAction(async () => runAppUpdate());
  }

  function handleCapsuleContextMenu(event: MouseEvent, item: CapsuleSummary) {
    event.preventDefault();
    setSelectedCapsulePath(item.path);
    setCapsuleMenu({
      x: Math.min(event.clientX, window.innerWidth - 240),
      y: Math.min(event.clientY, window.innerHeight - 230),
      item,
    });
  }

  function deleteCapsuleFromMenu(item: CapsuleSummary) {
    if (!window.confirm(`${t("deleteCapsule")}: ${item.summary_preview || item.capsule_id}?`)) return;
    void runTitlebarAction(async () => {
      await deleteCapsule(item.path);
      await refresh({ force: true });
      return { message: t("deleteCapsule") };
    });
  }

  function copyCapsulePath(item: CapsuleSummary) {
    void navigator.clipboard.writeText(item.path).then(
      () => reportMenuMessage(t("pathCopied")),
      (err) => reportMenuError(err),
    );
  }

  function openCapsuleFolderFromMenu(item: CapsuleSummary) {
    void runTitlebarAction(async () => {
      const result = await openCapsuleFolder(item.path);
      return { message: result.message || t("folderOpened") };
    });
  }

  function openCapsuleWithFromMenu(item: CapsuleSummary) {
    void runTitlebarAction(async () => {
      const result = await openCapsuleExternal(item.path);
      return { message: result.message || t("fileOpened") };
    });
  }

  const resolvedTheme = resolveTheme(theme, osDark);
  const themeStyle = {
    "--sidebar-width": `${sidebarWidth}px`,
    ...(resolvedTheme
      ? {
          "--focus-border-color": resolvedTheme.colors.focus_border_color,
          "--selection-bg-color": resolvedTheme.colors.selection_bg_color,
          "--selection-fg-color": resolvedTheme.colors.selection_fg_color,
          "--app-bg-color": resolvedTheme.colors.app_bg_color,
          "--sidebar-bg-color": resolvedTheme.colors.sidebar_bg_color,
          "--panel-bg-color": resolvedTheme.colors.panel_bg_color,
          "--text-color": resolvedTheme.colors.text_color,
          "--agent-codex-color": resolvedTheme.colors.codex_color,
          "--agent-claude-color": resolvedTheme.colors.claude_color,
        }
      : {}),
  } as CSSProperties;

  const pageTitle =
    active === "capsules" ? t("capsules") : t(navTabs.find((tab) => tab.id === active)?.labelKey ?? "overview");
  const guiDark = resolvedTheme?.dark ?? false;

  return (
    <div
      className={`app codex-light ${guiDark ? "gui-dark" : ""} ${sidebarCollapsed ? "sidebar-collapsed" : ""} ${
        sidebarResizing ? "sidebar-resizing" : ""
      }`}
      style={themeStyle}
    >
      <AppTitlebar
        t={t}
        snapshot={snapshot}
        sidebarCollapsed={sidebarCollapsed}
        refreshing={refreshing}
        statusMessage={titleStatus}
        onToggleSidebar={() => setSidebarCollapsed((collapsed) => !collapsed)}
        onRefresh={() => void refresh({ force: true })}
        onSettings={() => setSettingsOpen(true)}
        onCheckpoint={handleCheckpoint}
        onOpenLogs={handleOpenLogs}
        onAddAccount={handleAddAccount}
        onAccountStatus={handleAccountStatus}
        onDoctor={handleDoctor}
        onReinstallHooks={handleReinstallHooks}
        onRestartDaemon={handleRestartDaemon}
        onOpenGithub={handleOpenGithub}
      />
      <aside className="sidebar">
        <div className="sidebar-top">
          <div className="sidebar-tools">
            <button className="tool-button" title={t("searchSettings")}>
              <Search size={17} />
            </button>
          </div>
          <nav aria-label={t("navigation")}>
            {navTabs.map((tab) => {
              const Icon = tab.icon;
              return (
                <button
                  key={tab.id}
                  className={active === tab.id ? "nav active" : "nav"}
                  onClick={() => setActive(tab.id)}
                  title={t(tab.labelKey)}
                >
                  <Icon size={18} />
                  <span>{t(tab.labelKey)}</span>
                </button>
              );
            })}
          </nav>
        </div>
        {snapshot && (
          <CapsuleNavigator
            items={snapshot.capsules.items}
            selectedPath={selectedCapsulePath}
            openAgents={openAgents}
            openProjectKeys={openProjectKeys}
            query={capsuleQuery}
            stateFilter={capsuleStateFilter}
            onQueryChange={setCapsuleQuery}
            onStateFilterChange={setCapsuleStateFilter}
            onToggleAgent={toggleAgent}
            onToggleProject={toggleProject}
            onCapsuleContextMenu={handleCapsuleContextMenu}
            onSelectCapsule={(path) => {
              setSelectedCapsulePath(path);
              setActive("capsules");
            }}
            t={t}
          />
        )}
        <div className="sidebar-version">
          {updateStatus?.update_available && updateStatus.latest ? (
            <button className="sidebar-update" title={t("updateBadgeTitle")} onClick={handleUpdateNow}>
              <span className="update-dot" aria-hidden="true" />
              <span>{appVersion ? `v${appVersion} → ${updateStatus.latest}` : updateStatus.latest}</span>
            </button>
          ) : (
            appVersion && <span className="sidebar-version-text">v{appVersion}</span>
          )}
        </div>
        <button
          className="sidebar-resizer"
          aria-label="Resize sidebar"
          title="Resize sidebar"
          onPointerDown={startSidebarResize}
        />
      </aside>
      <main>
        <header className="content-header">
          <div>
            <h2>{pageTitle}</h2>
          </div>
          {snapshot && active === "overview" && <OverviewHeaderStats snapshot={snapshot} t={t} />}
        </header>
        {error && (
          <section className="banner error">
            {t("failedDashboard")}: {error}
          </section>
        )}
        {refreshing && snapshot && <section className="banner">{t("refreshingState")}</section>}
        {!snapshot && !error && <section className="empty">{t("loadingState")}</section>}
        {snapshot && active === "overview" && <Overview snapshot={snapshot} t={t} />}
        {snapshot && active === "capsules" && (
          <Capsules
            initial={snapshot.capsules}
            selectedPath={selectedCapsulePath}
            onSelectedPathChange={setSelectedCapsulePath}
            onChanged={() => refresh({ force: true })}
            onDeleteCapsule={deleteCapsuleFromMenu}
            onCopyPath={copyCapsulePath}
            onOpenFolder={openCapsuleFolderFromMenu}
            onOpenWith={openCapsuleWithFromMenu}
            t={t}
          />
        )}
        {snapshot && active === "usage" && <Usage t={t} />}
        {snapshot && active === "account" && <Account snapshot={snapshot} t={t} />}
        {snapshot && active === "integration" && <Integration initial={snapshot} t={t} />}
      </main>
      <SettingsModal
        open={settingsOpen}
        snapshot={snapshot}
        onClose={() => setSettingsOpen(false)}
        onThemeChanged={() => refresh({ force: true })}
        t={t}
      />
      <CapsuleContextMenu
        state={capsuleMenu}
        t={t}
        onClose={() => setCapsuleMenu(null)}
        onDelete={deleteCapsuleFromMenu}
        onCopyPath={copyCapsulePath}
        onOpenFolder={openCapsuleFolderFromMenu}
        onOpenWith={openCapsuleWithFromMenu}
      />
      <LimitSwitchModal
        alert={limitAlert}
        t={t}
        onSwitch={handleLimitSwitch}
        onDismiss={handleLimitDismiss}
      />
    </div>
  );
}
