import { useEffect, useState } from "react";
import { getAccountReport } from "../api";
import type { AccountReport, AccountWindow, CheckRow, DashboardSnapshot } from "../types";
import type { Translator } from "../i18n";

function StatusRow({ row }: { row: CheckRow }) {
  return (
    <div className={`overview-status-row ${row.status}`}>
      <span className="overview-status-dot" aria-hidden="true" />
      <strong>{row.label}</strong>
      <span className="overview-status-message">{row.message}</span>
      {row.path && <code>{row.path}</code>}
    </div>
  );
}

function pct(value: number) {
  return `${Math.round(value)}%`;
}

function LimitBar({
  agent,
  label,
  value,
  t,
}: {
  agent: "codex" | "claude";
  label: string;
  value?: AccountWindow | null;
  t: Translator;
}) {
  const used = value ? Math.max(0, Math.min(100, value.used_percent)) : 0;
  return (
    <div className={`limit-row ${agent}`}>
      <strong>{label}</strong>
      <div className="usage-bar" aria-hidden="true">
        <span style={{ width: `${used}%` }} />
      </div>
      <span>{value ? `${pct(value.remaining_percent)} ${t("left")}` : t("noSample")}</span>
    </div>
  );
}

export default function Overview({ snapshot, t }: { snapshot: DashboardSnapshot; t: Translator }) {
  const [accounts, setAccounts] = useState<AccountReport | null>(null);
  const [loadingAccounts, setLoadingAccounts] = useState(true);

  useEffect(() => {
    getAccountReport()
      .then(setAccounts)
      .catch(() => setAccounts(null))
      .finally(() => setLoadingAccounts(false));
  }, []);

  const topRows = [
    snapshot.daemon,
    snapshot.autostart,
    snapshot.codex_hooks,
    snapshot.codex_config,
    snapshot.claude_settings,
    snapshot.ipc,
    snapshot.store,
  ];
  const issueCount = topRows.filter((row) => row.status !== "ok").length;

  return (
    <div className="overview-view">
      <section className="overview-section">
        <div className="overview-section-title">
          <h3>{t("agentLimits")}</h3>
          <span>{t("agentLimitsHelp")}</span>
        </div>
        {loadingAccounts && <div className="overview-inline-loading">{t("loadingAccounts")}</div>}
        <div className="overview-limits">
          <LimitBar agent="claude" label={`${t("claude")} 5h`} value={accounts?.claude.five_hour} t={t} />
          <LimitBar agent="claude" label={`${t("claude")} ${t("weekly")}`} value={accounts?.claude.weekly} t={t} />
          <LimitBar agent="codex" label={`${t("codex")} 5h`} value={accounts?.codex.five_hour} t={t} />
          <LimitBar agent="codex" label={`${t("codex")} ${t("weekly")}`} value={accounts?.codex.weekly} t={t} />
        </div>
      </section>
      <section className="overview-section">
        <div className="overview-section-title">
          <h3>{t("systemStatus")}</h3>
          {issueCount > 0 && <span>{`${issueCount} ${t("attention")}`}</span>}
        </div>
        <div className="overview-status-list">
        {topRows.map((row) => (
          <StatusRow key={row.id} row={row} />
        ))}
        </div>
      </section>
    </div>
  );
}
