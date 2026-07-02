import { useEffect, useMemo, useState } from "react";
import { getUsageReport } from "../api";
import type { UsageGroup, UsageReport } from "../types";
import type { Translator } from "../i18n";

type Mode = "day" | "project" | "model" | "source";

const modes: Array<{ id: Mode; labelKey: string }> = [
  { id: "day", labelKey: "day" },
  { id: "project", labelKey: "project" },
  { id: "model", labelKey: "model" },
  { id: "source", labelKey: "source" },
];

function formatTokens(tokens: number) {
  if (tokens >= 1_000_000_000) return `${(tokens / 1_000_000_000).toFixed(2)}B`;
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(2)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}K`;
  return String(tokens);
}

function formatCost(cost: number) {
  return `$${cost.toFixed(2)}`;
}

function rowsFor(report: UsageReport, mode: Mode) {
  switch (mode) {
    case "day":
      return report.by_day;
    case "project":
      return report.by_project;
    case "model":
      return report.by_model;
    case "source":
      return report.by_source;
  }
}

function UsageBar({ group, max }: { group: UsageGroup; max: number }) {
  const pct = max > 0 ? Math.max(2, Math.round((group.tokens.total / max) * 100)) : 0;
  return (
    <div className={`usage-bar ${group.key}`}>
      <span style={{ width: `${pct}%` }} />
    </div>
  );
}

export default function Usage({ t }: { t: Translator }) {
  const [report, setReport] = useState<UsageReport | null>(null);
  const [mode, setMode] = useState<Mode>("day");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    void loadUsage(false);
  }, []);

  async function loadUsage(force: boolean) {
    setLoading(true);
    setError(null);
    try {
      setReport(await getUsageReport({ force }));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  const rows = report ? rowsFor(report, mode) : [];
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
      <section className="agent-bars">
        {report.by_source.map((source) => (
          <article key={source.key}>
            <div>
              <strong>{source.key}</strong>
              <span>{formatTokens(source.tokens.total)}</span>
            </div>
            <UsageBar group={source} max={report.total.tokens.total} />
          </article>
        ))}
      </section>
      <section className="panel">
        <div className="panel-title">
          <h3>{t("breakdown")}</h3>
          <div className="actions">
            <div className="segmented">
              {modes.map((item) => (
                <button
                  key={item.id}
                  className={mode === item.id ? "active" : ""}
                  onClick={() => setMode(item.id)}
                >
                  {t(item.labelKey)}
                </button>
              ))}
            </div>
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
                <span>{row.key}</span>
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
