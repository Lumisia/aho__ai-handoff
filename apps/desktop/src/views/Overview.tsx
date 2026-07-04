import { useEffect, useState } from "react";
import { getAccountReport, getUsageReport } from "../api";
import TokenUsageChart, { type UsageBreakdownMode, type UsageChartView } from "../components/TokenUsageChart";
import type { AccountReport, AccountWindow, CheckRow, DashboardSnapshot, UsageReport } from "../types";
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

function resetText(value: AccountWindow | null | undefined) {
  if (!value?.resets_at) return null;
  return new Date(value.resets_at * 1000).toLocaleString(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
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
  const reset = resetText(value);
  return (
    <div className={`limit-row ${agent}`}>
      <strong>{label}</strong>
      <div className="usage-bar" aria-hidden="true">
        <span style={{ width: `${used}%` }} />
      </div>
      <span className="limit-right">
        <span className="limit-pct">{value ? `${pct(value.remaining_percent)} ${t("left")}` : t("noSample")}</span>
        {reset && (
          <small className="limit-reset">
            {t("resetsAt")} {reset}
          </small>
        )}
      </span>
    </div>
  );
}

export default function Overview({ snapshot, t }: { snapshot: DashboardSnapshot; t: Translator }) {
  const [accounts, setAccounts] = useState<AccountReport | null>(null);
  const [usage, setUsage] = useState<UsageReport | null>(null);
  const [loadingAccounts, setLoadingAccounts] = useState(true);
  const [loadingUsage, setLoadingUsage] = useState(true);
  const [usageMode, setUsageMode] = useState<UsageBreakdownMode>("day");
  const [usageView, setUsageView] = useState<UsageChartView>("3d");

  useEffect(() => {
    // force: query the active saved slot's own usage so the overview shows the
    // active Claude/Codex 5h+weekly limits (statusline samples alone are often
    // absent). Only the active saved slot is hit — never a bare live credential.
    getAccountReport({ force: true })
      .then(setAccounts)
      .catch(() => setAccounts(null))
      .finally(() => setLoadingAccounts(false));

    getUsageReport()
      .then(setUsage)
      .catch(() => setUsage(null))
      .finally(() => setLoadingUsage(false));
  }, []);

  const topRows = [
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
        {loadingUsage && <div className="overview-inline-loading">{t("loadingUsage")}</div>}
        {usage && (
          <TokenUsageChart
            report={usage}
            mode={usageMode}
            view={usageView}
            onModeChange={setUsageMode}
            onViewChange={setUsageView}
            t={t}
          />
        )}
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
