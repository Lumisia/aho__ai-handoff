import ClaudeIcon from "@lobehub/icons/es/Claude";
import OpenAIIcon from "@lobehub/icons/es/OpenAI";
import { useEffect, useMemo, useState } from "react";
import { getUsageReport } from "../api";
import TokenUsageChart, {
  agentForKey,
  formatCost,
  formatTokens,
  periodDays,
  rowsForUsageMode,
  type UsageBreakdownMode,
  type UsageChartView,
  type UsagePeriod,
} from "../components/TokenUsageChart";
import type { UsageGroup, UsageReport } from "../types";
import type { Translator } from "../i18n";

function UsageBar({ group, max }: { group: UsageGroup; max: number }) {
  const pct = max > 0 ? Math.max(2, Math.round((group.tokens.total / max) * 100)) : 0;
  const agent = agentForKey(group.key);
  return (
    <div className={`usage-bar ${agent}`}>
      <span style={{ width: `${pct}%` }} />
    </div>
  );
}

function ModelIcon({ name }: { name: string }) {
  const agent = agentForKey(name);
  if (agent === "claude") {
    return (
      <span className="model-icon claude" aria-hidden="true">
        <ClaudeIcon size={15} />
      </span>
    );
  }
  if (agent === "codex") {
    return (
      <span className="model-icon codex" aria-hidden="true">
        <OpenAIIcon size={15} />
      </span>
    );
  }
  return <span className="model-icon other" aria-hidden="true" />;
}

function UsageKey({ row, mode }: { row: UsageGroup; mode: UsageBreakdownMode }) {
  if (mode !== "model") return <span>{row.key}</span>;
  return (
    <span className="usage-key-with-icon">
      <ModelIcon name={row.key} />
      <span>{row.key}</span>
    </span>
  );
}

export default function Usage({ t }: { t: Translator }) {
  const [report, setReport] = useState<UsageReport | null>(null);
  const [mode, setMode] = useState<UsageBreakdownMode>("day");
  const [chartView, setChartView] = useState<UsageChartView>("3d");
  const [period, setPeriod] = useState<UsagePeriod>("month");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    void loadUsage(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [period]);

  async function loadUsage(force: boolean) {
    setLoading(true);
    setError(null);
    try {
      setReport(await getUsageReport({ force, days: periodDays(period) }));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  const rows = useMemo(() => {
    if (!report) return [];
    const base = rowsForUsageMode(report, mode);
    if (mode === "day") return base;
    // Project / model / source: rank highest usage first.
    return [...base].sort((a, b) => b.tokens.total - a.tokens.total || a.key.localeCompare(b.key));
  }, [mode, report]);
  const max = useMemo(() => Math.max(0, ...rows.map((row) => row.tokens.total)), [rows]);

  if (error) {
    return <section className="banner error">{t("failedUsage")}: {error}</section>;
  }

  if (loading) {
    return <section className="loading-screen">{t("loadingUsage")}</section>;
  }

  if (!report) {
    return <section className="empty">{t("noUsage")}</section>;
  }

  return (
    <div className="usage-layout">
      <section className="usage-summary">
        <div>
          <span>{t("totalTokens")}</span>
          <strong>{formatTokens(report.total.tokens.total)}</strong>
        </div>
        <div>
          <span>{t("estimatedCost")}</span>
          <strong>{formatCost(report.total.cost_usd)}</strong>
        </div>
        <div>
          <span>{t("unpriced")}</span>
          <strong>{formatTokens(report.total.unpriced_tokens)}</strong>
        </div>
      </section>
      <TokenUsageChart
        report={report}
        mode={mode}
        view={chartView}
        period={period}
        onModeChange={setMode}
        onViewChange={setChartView}
        onPeriodChange={setPeriod}
        t={t}
      />
      <section className="panel">
        <div className="panel-title">
          <h3>{t("breakdown")}</h3>
          <div className="actions">
            <button className="ghost" onClick={() => void loadUsage(true)}>
              {t("refresh")}
            </button>
          </div>
        </div>
        <div className="usage-table">
          <div className="table-row head">
            <span>{t(mode)}</span>
            <span>{t("tokens")}</span>
            <span>{t("cost")}</span>
            <span>{t("events")}</span>
          </div>
          {rows.map((row) => (
            <div className="usage-row" key={row.key}>
              <div className="table-row">
                <UsageKey row={row} mode={mode} />
                <strong>{formatTokens(row.tokens.total)}</strong>
                <span>{formatCost(row.cost_usd)}</span>
                <span>{row.events}</span>
              </div>
              <UsageBar group={row} max={max} />
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
