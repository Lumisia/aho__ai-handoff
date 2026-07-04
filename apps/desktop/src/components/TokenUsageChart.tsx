import type { CSSProperties } from "react";
import type { Translator } from "../i18n";
import type { UsageGroup, UsageReport } from "../types";

export type UsageBreakdownMode = "day" | "project" | "model" | "source";
export type UsageChartView = "2d" | "3d";

export const usageBreakdownModes: Array<{ id: UsageBreakdownMode; labelKey: string }> = [
  { id: "day", labelKey: "day" },
  { id: "project", labelKey: "project" },
  { id: "model", labelKey: "model" },
  { id: "source", labelKey: "source" },
];

interface ChartSegment {
  key: string;
  tokens: number;
  agent: "codex" | "claude" | "other";
}

interface DayStack {
  day: string;
  total: number;
  cost: number;
  events: number;
  segments: ChartSegment[];
}

interface RankBar {
  key: string;
  tokens: number;
  cost: number;
  events: number;
  agent: "codex" | "claude" | "other";
}

export function formatTokens(tokens: number) {
  if (tokens >= 1_000_000_000) return `${(tokens / 1_000_000_000).toFixed(2)}B`;
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(2)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}K`;
  return String(tokens);
}

export function formatCost(cost: number) {
  return `$${cost.toFixed(2)}`;
}

export function rowsForUsageMode(report: UsageReport, mode: UsageBreakdownMode): UsageGroup[] {
  switch (mode) {
    case "day":
      return report.by_day;
    case "project":
      return report.recent_by_project;
    case "model":
      return report.recent_by_model;
    case "source":
      return report.recent_by_source;
  }
}

export function agentForKey(key: string): "codex" | "claude" | "other" {
  const lower = key.toLowerCase();
  if (lower.includes("claude")) return "claude";
  if (lower.includes("codex") || lower.includes("gpt") || lower.includes("openai")) return "codex";
  return "other";
}

function colorForAgent(agent: "codex" | "claude" | "other") {
  if (agent === "codex") return "var(--token-codex-fixed)";
  if (agent === "claude") return "var(--token-claude-fixed)";
  return "var(--accent-color)";
}

function shortDay(day: string) {
  const [, month, date] = day.split("-");
  return month && date ? `${month}.${date}` : day;
}

function compareByTokens(view: UsageChartView) {
  return (a: { key?: string; total?: number; tokens?: number }, b: { key?: string; total?: number; tokens?: number }) => {
    const aTokens = a.total ?? a.tokens ?? 0;
    const bTokens = b.total ?? b.tokens ?? 0;
    const tokenDelta = view === "2d" ? bTokens - aTokens : aTokens - bTokens;
    return tokenDelta || (a.key ?? "").localeCompare(b.key ?? "");
  };
}

function buildDayStacks(report: UsageReport, view: UsageChartView): DayStack[] {
  const byDay = new Map(report.by_day.map((group) => [group.key, group]));
  const byDaySource = new Map<string, ChartSegment[]>();
  for (const item of report.by_day_source) {
    const current = byDaySource.get(item.day) ?? [];
    current.push({
      key: item.source,
      tokens: item.tokens.total,
      agent: agentForKey(item.source),
    });
    byDaySource.set(item.day, current);
  }

  return report.by_day
    .slice()
    .map((group) => {
      const segments = (byDaySource.get(group.key) ?? [])
        .filter((segment) => segment.tokens > 0)
        .sort((a, b) => b.tokens - a.tokens || a.key.localeCompare(b.key));
      const day = byDay.get(group.key) ?? group;
      return {
        day: group.key,
        total: day.tokens.total,
        cost: day.cost_usd,
        events: day.events,
        segments,
      };
    })
    .sort(compareByTokens(view));
}

function buildRankBars(report: UsageReport, mode: UsageBreakdownMode, view: UsageChartView): RankBar[] {
  const bars = rowsForUsageMode(report, mode)
    .filter((group) => group.tokens.total > 0)
    .map((group) => ({
      key: group.key,
      tokens: group.tokens.total,
      cost: group.cost_usd,
      events: group.events,
      agent: agentForKey(group.key),
    }))
    .sort(compareByTokens(view));
  return view === "3d" ? bars.slice(-14) : bars.slice(0, 14);
}

function DayChart({ stacks, view }: { stacks: DayStack[]; view: UsageChartView }) {
  const max = Math.max(0, ...stacks.map((stack) => stack.total));
  if (view === "3d") {
    return (
      <div className="token-plot token-plot-3d" aria-label="30 day token chart">
        <div className="token-3d-grid">
          {stacks.map((stack, index) => {
            let offset = 0;
            return (
              <div className="token-3d-column" key={stack.day} title={`${stack.day} - ${formatTokens(stack.total)}`}>
                {stack.segments.map((segment) => {
                  const height = max > 0 ? Math.max(3, Math.round((segment.tokens / max) * 118)) : 0;
                  const segmentOffset = offset;
                  offset += height;
                  return (
                    <span
                      className={`token-3d-segment ${segment.agent}`}
                      key={`${stack.day}-${segment.key}`}
                      style={
                        {
                          "--token-color": colorForAgent(segment.agent),
                          "--token-delay": `${Math.min(index * 18, 360)}ms`,
                          "--token-offset": `${segmentOffset}px`,
                          height: `${height}px`,
                        } as CSSProperties
                      }
                    />
                  );
                })}
              </div>
            );
          })}
        </div>
      </div>
    );
  }

  return (
    <div className="token-plot token-plot-2d" aria-label="30 day token chart">
      <div className="token-2d-bars">
        {stacks.map((stack, index) => {
          const height = max > 0 ? Math.max(3, Math.round((stack.total / max) * 100)) : 0;
          return (
            <div className="token-day-column" key={stack.day} title={`${stack.day} - ${formatTokens(stack.total)}`}>
              <div
                className="token-stack"
                style={
                  {
                    height: `${height}%`,
                    "--token-delay": `${Math.min(index * 18, 360)}ms`,
                  } as CSSProperties
                }
              >
                {stack.segments.map((segment) => (
                  <span
                    className={`token-stack-segment ${segment.agent}`}
                    key={`${stack.day}-${segment.key}`}
                    style={
                      {
                        "--token-color": colorForAgent(segment.agent),
                        flexGrow: segment.tokens,
                      } as CSSProperties
                    }
                  />
                ))}
              </div>
              <small>{shortDay(stack.day)}</small>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function RankChart({ bars, view }: { bars: RankBar[]; view: UsageChartView }) {
  const max = Math.max(0, ...bars.map((bar) => bar.tokens));
  if (view === "3d") {
    return (
      <div className="token-plot token-plot-3d compact-rank" aria-label="ranked token chart">
        <div className="token-3d-grid rank-grid">
          {bars.map((bar, index) => {
            const height = max > 0 ? Math.max(5, Math.round((bar.tokens / max) * 118)) : 0;
            return (
              <div className="token-3d-column rank-column" key={bar.key} title={`${bar.key} - ${formatTokens(bar.tokens)}`}>
                <span
                  className={`token-3d-segment ${bar.agent}`}
                  style={
                    {
                      "--token-color": colorForAgent(bar.agent),
                      "--token-delay": `${Math.min(index * 24, 360)}ms`,
                      "--token-offset": "0px",
                      height: `${height}px`,
                    } as CSSProperties
                  }
                />
              </div>
            );
          })}
        </div>
      </div>
    );
  }

  return (
    <div className="token-rank-list">
      {bars.map((bar, index) => {
        const width = max > 0 ? Math.max(3, Math.round((bar.tokens / max) * 100)) : 0;
        return (
          <div className="token-rank-row" key={bar.key}>
            <span>{bar.key}</span>
            <div className="token-rank-track">
              <i
                className={`token-rank-fill ${bar.agent}`}
                style={
                  {
                    "--token-color": colorForAgent(bar.agent),
                    "--token-delay": `${Math.min(index * 24, 360)}ms`,
                    width: `${width}%`,
                  } as CSSProperties
                }
              />
            </div>
            <strong>{formatTokens(bar.tokens)}</strong>
          </div>
        );
      })}
    </div>
  );
}

export default function TokenUsageChart({
  report,
  mode,
  view,
  onModeChange,
  onViewChange,
  t,
}: {
  report: UsageReport;
  mode: UsageBreakdownMode;
  view: UsageChartView;
  onModeChange: (mode: UsageBreakdownMode) => void;
  onViewChange: (view: UsageChartView) => void;
  t: Translator;
}) {
  const dayStacks = buildDayStacks(report, view);
  const rankBars = buildRankBars(report, mode, view);
  const activeDays = dayStacks.filter((stack) => stack.total > 0).length;
  const monthTokens = dayStacks.reduce((sum, stack) => sum + stack.total, 0);
  const monthCost = dayStacks.reduce((sum, stack) => sum + stack.cost, 0);
  const bestDay = dayStacks.reduce<DayStack | null>(
    (best, stack) => (best === null || stack.total > best.total ? stack : best),
    null,
  );
  const hasData = mode === "day" ? dayStacks.some((stack) => stack.total > 0) : rankBars.length > 0;

  return (
    <section className="token-usage-card">
      <div className="token-chart-head">
        <div>
          <h3>{t("tokenUsage")}</h3>
          <span>{mode === "day" ? t("monthWindow") : t(mode)}</span>
        </div>
        <div className="token-chart-controls">
          <div className="segmented token-category-tabs" aria-label={t("breakdown")}>
            {usageBreakdownModes.map((item) => (
              <button
                key={item.id}
                className={mode === item.id ? "active" : ""}
                onClick={() => onModeChange(item.id)}
              >
                {t(item.labelKey)}
              </button>
            ))}
          </div>
          <div className="segmented token-view-tabs" aria-label={t("chooseValue")}>
            <button className={view === "2d" ? "active" : ""} onClick={() => onViewChange("2d")}>
              {t("chart2d")}
            </button>
            <button className={view === "3d" ? "active" : ""} onClick={() => onViewChange("3d")}>
              {t("chart3d")}
            </button>
          </div>
        </div>
      </div>

      <div className="token-chart-stats">
        <div>
          <strong>{formatCost(monthCost)}</strong>
          <span>{t("estimatedCost")}</span>
        </div>
        <div>
          <strong>{formatTokens(monthTokens)}</strong>
          <span>{t("tokens")} · {activeDays} {t("activeDays")}</span>
        </div>
        <div>
          <strong>{bestDay ? formatTokens(bestDay.total) : "0"}</strong>
          <span>{t("bestDay")} {bestDay ? shortDay(bestDay.day) : "-"}</span>
        </div>
      </div>

      <div className="token-legend" aria-hidden="true">
        <span><i className="codex" />GPT/Codex</span>
        <span><i className="claude" />Claude</span>
      </div>

      {hasData ? (
        mode === "day" ? <DayChart stacks={dayStacks} view={view} /> : <RankChart bars={rankBars} view={view} />
      ) : (
        <div className="empty token-chart-empty">{t("chartEmpty")}</div>
      )}
    </section>
  );
}
