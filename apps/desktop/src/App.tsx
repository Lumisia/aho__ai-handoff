import {
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
  ScrollText,
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
  createCheckpoint,
  deleteCapsule,
  ensureDaemonRunning,
  getAccountReport,
  getDashboardSnapshot,
  getTheme,
  openCapsuleExternal,
  openCapsuleFolder,
  openLogsFolder,
  openProjectGithub,
  pollAccountLogin,
  reinstallHooks,
  runDoctor,
  startAccountLogin,
} from "./api";
import { createTranslator, normalizeLanguage } from "./i18n";
import type { CSSProperties, MouseEvent, PointerEvent as ReactPointerEvent, ReactNode } from "react";
import type { AccountLoginSession, CapsuleSummary, DashboardSnapshot, ThemeReport } from "./types";
import type { Translator } from "./i18n";
import Account from "./views/Account";
import Capsules from "./views/Capsules";
import Integration from "./views/Integration";
import Logs from "./views/Logs";
import Overview from "./views/Overview";
import SettingsView from "./views/Settings";
import Usage from "./views/Usage";

type Tab = "overview" | "capsules" | "usage" | "account" | "integration" | "settings" | "logs";
type AgentName = "Codex" | "Claude";

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
  { id: "logs", labelKey: "logs", icon: ScrollText },
];

function displayAgent(source: string): AgentName {
  return source.toLowerCase().includes("claude") ? "Claude" : "Codex";
}

function targetAgent(target: string) {
  return target.toLowerCase().includes("claude") ? "Claude" : "Codex";
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
    const agent = tree.find((node) => node.agent === displayAgent(item.source_agent)) ?? tree[0];
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
  const cls = agent === "Codex" ? "agent-logo codex" : "agent-logo claude";
  return (
    <span className={cls} aria-hidden="true">
      {agent === "Codex" ? <OpenAIIcon size={16} /> : <ClaudeIcon.Color size={16} />}
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
  const daemonStatus = snapshot?.daemon.status ?? "unknown";
  const context = snapshot
    ? `${t("pending")} ${pending} - daemon ${daemonStatus}`
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
        <span className={`health-dot ${daemonStatus === "ok" ? "ok" : daemonStatus === "warning" ? "warn" : ""}`} />
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

function CapsuleNavigator({
  items,
  selectedPath,
  openAgents,
  openProjectKeys,
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
  onToggleAgent: (agent: AgentName) => void;
  onToggleProject: (key: string) => void;
  onSelectCapsule: (path: string) => void;
  onCapsuleContextMenu: (event: MouseEvent, item: CapsuleSummary) => void;
  t: Translator;
}) {
  const tree = useMemo(() => buildCapsuleTree(items), [items]);

  return (
    <section className="sidebar-capsules" aria-label={t("capsules")}>
      <div className="sidebar-section-title">
        <FolderKanban size={15} />
        <span>{t("capsules")}</span>
      </div>
      {items.length === 0 && <div className="sidebar-empty">{t("noCapsules")}</div>}
      {tree.map((agent) => (
        <div className={`side-agent ${agent.agent.toLowerCase()}`} key={agent.agent}>
          <button className="side-agent-row" onClick={() => onToggleAgent(agent.agent)}>
            <span className="agent-title">
              {openAgents.includes(agent.agent) ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
              <AgentLogo agent={agent.agent} />
              <strong>{agent.agent === "Codex" ? t("codex") : t("claude")}</strong>
            </span>
            <span className="side-count">{agent.count}</span>
          </button>
          {openAgents.includes(agent.agent) && agent.projects.map((project) => {
            const key = `${agent.agent}-${project.project_id}`;
            const open = openProjectKeys.includes(key);
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
  const refreshSeq = useRef(0);
  const seededProjectRef = useRef(false);
  const sidebarDragOffset = useRef(0);
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

  const themeStyle = {
    "--sidebar-width": `${sidebarWidth}px`,
    ...(theme
      ? {
          "--codex-color": theme.codex_color,
          "--claude-color": theme.claude_color,
          "--focus-border-color": theme.focus_border_color,
          "--selection-bg-color": theme.selection_bg_color,
          "--selection-fg-color": theme.selection_fg_color,
          "--app-bg-color": theme.app_bg_color,
          "--sidebar-bg-color": theme.sidebar_bg_color,
          "--panel-bg-color": theme.panel_bg_color,
          "--text-color": theme.text_color,
        }
      : {}),
  } as CSSProperties;

  const pageTitle =
    active === "capsules" ? t("capsules") : t(navTabs.find((tab) => tab.id === active)?.labelKey ?? "overview");
  const guiDark = theme?.preset === "dark";

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
        {snapshot && active === "logs" && <Logs t={t} />}
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
    </div>
  );
}
