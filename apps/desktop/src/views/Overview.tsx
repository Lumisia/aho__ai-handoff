import { useEffect, useState } from "react";
import { getAccountReport, getUsageReport } from "../api";
import TokenUsageChart, {
  periodDays,
  type UsageBreakdownMode,
  type UsageChartView,
  type UsagePeriod,
} from "../components/TokenUsageChart";
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

function clockText(secs: number | null | undefined) {
  if (!secs) return null;
  return new Date(secs * 1000).toLocaleString(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function resetText(value: AccountWindow | null | undefined) {
  return clockText(value?.resets_at);
}

/// Projected exhaust time, shown only when the window runs out BEFORE it
/// resets — the case worth acting on. Otherwise the reset line is the story.
function exhaustText(value: AccountWindow | null | undefined) {
  if (!value?.projected_exhaust_at || !value.resets_at) return null;
  if (value.projected_exhaust_at >= value.resets_at) return null;
  return clockText(value.projected_exhaust_at);
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
  const exhaust = exhaustText(value);
  const rate = value?.burn_rate_per_hour;
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
        {exhaust && (
          <small className="limit-exhaust" title={rate ? `≈ ${rate.toFixed(1)}%/h` : undefined}>
            {t("runsOutAt")} ~{exhaust}
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
  const [usagePeriod, setUsagePeriod] = useState<UsagePeriod>("month");

  useEffect(() => {
    // force: query the active saved slot's own usage so the overview shows the
    // active Claude/Codex 5h+weekly limits (statusline samples alone are often
    // absent). Only the active saved slot is hit — never a bare live credential.
    getAccountReport({ force: true })
      .then(setAccounts)
      .catch(() => setAccounts(null))
      .finally(() => setLoadingAccounts(false));
  }, []);

  useEffect(() => {
    setLoadingUsage(true);
    getUsageReport({ days: periodDays(usagePeriod) })
      .then(setUsage)
      .catch(() => setUsage(null))
      .finally(() => setLoadingUsage(false));
  }, [usagePeriod]);

  const topRows = [
    snapshot.codex_hooks,
    snapshot.codex_config,
    snapshot.claude_settings,
    snapshot.ipc,
    snapshot.store,
  ];
  const issueCount = topRows.filter((row) => row.status !== "ok").length;
  const claudeLimitsVisible = Boolean(accounts?.claude.active);
  const codexLimitsVisible = Boolean(accounts?.codex.active);
  const anyLimitsVisible = claudeLimitsVisible || codexLimitsVisible;

  return (
    <div className="overview-view">
      <section className="overview-section">
        <div className="overview-section-title">
          <h3>{t("agentLimits")}</h3>
          <span>{t("agentLimitsHelp")}</span>
        </div>
        {loadingAccounts && <div className="overview-inline-loading">{t("loadingAccounts")}</div>}
        {!loadingAccounts && !anyLimitsVisible && <div className="overview-inline-loading">{t("limitsNeedAccount")}</div>}
        <div className="overview-limits">
          {claudeLimitsVisible && <LimitBar agent="claude" label={`${t("claude")} 5h`} value={accounts?.claude.five_hour} t={t} />}
          {claudeLimitsVisible && <LimitBar agent="claude" label={`${t("claude")} ${t("weekly")}`} value={accounts?.claude.weekly} t={t} />}
          {codexLimitsVisible && <LimitBar agent="codex" label={`${t("codex")} 5h`} value={accounts?.codex.five_hour} t={t} />}
          {codexLimitsVisible && <LimitBar agent="codex" label={`${t("codex")} ${t("weekly")}`} value={accounts?.codex.weekly} t={t} />}
        </div>
      </section>
      <section className="overview-section">
        {loadingUsage && <div className="overview-inline-loading">{t("loadingUsage")}</div>}
        {usage && (
          <TokenUsageChart
            report={usage}
            mode={usageMode}
            view={usageView}
            period={usagePeriod}
            onModeChange={setUsageMode}
            onViewChange={setUsageView}
            onPeriodChange={setUsagePeriod}
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
