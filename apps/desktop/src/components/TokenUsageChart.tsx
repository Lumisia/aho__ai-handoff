import type { CSSProperties } from "react";
import type { Translator } from "../i18n";
import type { UsageGroup, UsageReport } from "../types";
import Contribution3DChart, { type IsoCell } from "./Contribution3DChart";

export type UsageBreakdownMode = "day" | "project" | "model" | "source";
export type UsageChartView = "2d" | "3d";
export type UsagePeriod = "month" | "quarter" | "year";

export const usageBreakdownModes: Array<{ id: UsageBreakdownMode; labelKey: string }> = [
  { id: "day", labelKey: "day" },
  { id: "project", labelKey: "project" },
  { id: "model", labelKey: "model" },
  { id: "source", labelKey: "source" },
];

export const usagePeriods: Array<{
  id: UsagePeriod;
  labelKey: string;
  windowKey: string;
  days: number;
}> = [
  { id: "month", labelKey: "period1m", windowKey: "window1m", days: 30 },
  { id: "quarter", labelKey: "period3m", windowKey: "window3m", days: 90 },
  { id: "year", labelKey: "period1y", windowKey: "window1y", days: 365 },
];

export function periodDays(period: UsagePeriod): number {
  return usagePeriods.find((p) => p.id === period)?.days ?? 30;
}

interface IsoGrid {
  cells: IsoCell[];
  cols: number;
  rows: number;
  maxTokens: number;
}

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

function buildDayStacks(report: UsageReport): DayStack[] {
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
    // Daily view is a timeline: always chronological, never ranked by tokens.
    .sort((a, b) => a.day.localeCompare(b.day));
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

function parseDay(day: string): Date {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, (m ?? 1) - 1, d ?? 1);
}

function isoDay(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function addDays(date: Date, n: number): Date {
  const c = new Date(date);
  c.setDate(c.getDate() + n);
  return c;
}

function diffDays(a: Date, b: Date): number {
  return Math.round((a.getTime() - b.getTime()) / 86_400_000);
}

// GitHub-style calendar: 7 rows (Sun..Sat), one column per week. Days with no
// usage stay as flat floor tiles; active days rise as agent-stacked cubes.
function dayCalendar(stacks: DayStack[]): IsoGrid {
  if (stacks.length === 0) return { cells: [], cols: 0, rows: 7, maxTokens: 0 };
  const byDay = new Map(stacks.map((stack) => [stack.day, stack]));
  const earliest = parseDay(stacks[0].day);
  const latest = parseDay(stacks[stacks.length - 1].day);
  const start = addDays(earliest, -earliest.getDay()); // Sunday on/before earliest
  const cols = Math.floor(diffDays(latest, start) / 7) + 1;
  const cells: IsoCell[] = [];
  let maxTokens = 0;
  for (let col = 0; col < cols; col++) {
    for (let row = 0; row < 7; row++) {
      const date = addDays(start, col * 7 + row);
      const key = isoDay(date);
      const stack = byDay.get(key);
      const total = stack?.total ?? 0;
      if (total > maxTokens) maxTokens = total;
      cells.push({
        col,
        row,
        label: shortDay(key),
        total,
        cost: stack?.cost ?? 0,
        events: stack?.events ?? 0,
        segments: (stack?.segments ?? []).map((segment) => ({
          key: segment.key,
          value: segment.tokens,
          agent: segment.agent,
        })),
      });
    }
  }
  return { cells, cols, rows: 7, maxTokens };
}

function DayChart({ stacks, view, t }: { stacks: DayStack[]; view: UsageChartView; t: Translator }) {
  const max = Math.max(0, ...stacks.map((stack) => stack.total));
  if (view === "3d") {
    const grid = dayCalendar(stacks);
    return (
      <Contribution3DChart
        cells={grid.cells}
        cols={grid.cols}
        rows={grid.rows}
        maxTokens={grid.maxTokens}
        t={t}
      />
    );
  }

  return (
    <div className="token-plot token-plot-2d" aria-label="daily token chart">
      <div
        className="token-2d-bars"
        style={{ gridTemplateColumns: `repeat(${Math.max(stacks.length, 1)}, minmax(0, 1fr))` }}
      >
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

function rankGrid(bars: RankBar[]): IsoGrid {
  const cells: IsoCell[] = bars.map((bar, index) => ({
    col: index,
    row: 0,
    label: bar.key,
    total: bar.tokens,
    cost: bar.cost,
    events: bar.events,
    segments: [{ key: bar.key, value: bar.tokens, agent: bar.agent }],
  }));
  return { cells, cols: cells.length, rows: 1, maxTokens: Math.max(0, ...bars.map((b) => b.tokens)) };
}

function RankChart({ bars, view, t }: { bars: RankBar[]; view: UsageChartView; t: Translator }) {
  const max = Math.max(0, ...bars.map((bar) => bar.tokens));
  if (view === "3d") {
    const grid = rankGrid(bars);
    return (
      <Contribution3DChart
        cells={grid.cells}
        cols={grid.cols}
        rows={grid.rows}
        maxTokens={grid.maxTokens}
        t={t}
      />
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
  period,
  onModeChange,
  onViewChange,
  onPeriodChange,
  t,
}: {
  report: UsageReport;
  mode: UsageBreakdownMode;
  view: UsageChartView;
  period: UsagePeriod;
  onModeChange: (mode: UsageBreakdownMode) => void;
  onViewChange: (view: UsageChartView) => void;
  onPeriodChange: (period: UsagePeriod) => void;
  t: Translator;
}) {
  const windowLabel = t(usagePeriods.find((p) => p.id === period)?.windowKey ?? "window1m");
  const dayStacks = buildDayStacks(report);
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
        <div className="token-chart-title">
          <h3>{t("tokenUsage")}</h3>
          <span>{windowLabel}</span>
          <div className="segmented token-period-tabs" aria-label={windowLabel}>
            {usagePeriods.map((item) => (
              <button
                key={item.id}
                className={period === item.id ? "active" : ""}
                onClick={() => onPeriodChange(item.id)}
              >
                {t(item.labelKey)}
              </button>
            ))}
          </div>
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
        mode === "day" ? (
          <DayChart stacks={dayStacks} view={view} t={t} />
        ) : (
          <RankChart bars={rankBars} view={view} t={t} />
        )
      ) : (
        <div className="empty token-chart-empty">{t("chartEmpty")}</div>
      )}
    </section>
  );
}
