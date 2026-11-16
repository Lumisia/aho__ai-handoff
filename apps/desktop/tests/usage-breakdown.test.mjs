import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { buildDayStacks, UsageBar } from "../src/components/TokenUsageChart.tsx";

function dayBar(day, sourceCounts, max) {
  const group = {
    key: day,
    tokens: { total: sourceCounts.reduce((sum, [, total]) => sum + total, 0) },
    cost_usd: 0.02,
    events: 3,
  };
  const by_day_source = sourceCounts.map(([source, total]) => ({ day, source, tokens: { total } }));
  const [stack] = buildDayStacks({ by_day: [group], by_day_source });
  return renderToStaticMarkup(createElement(UsageBar, { group, max, segments: stack.segments }));
}

test("daily breakdown shows Claude and Codex shares against the same scale", () => {
  const html = dayBar("2026-11-16", [["codex", 75], ["claude", 25]], 200);

  assert.match(html, /aria-label="2026-11-16 · GPT\/Codex: 75 · Claude: 25"/);
  assert.match(html, /width:37\.5%;background:var\(--token-codex-fixed\)/);
  assert.match(html, /width:12\.5%;background:var\(--token-claude-fixed\)/);
});

test("daily breakdown omits a provider segment when its usage is zero", () => {
  const html = dayBar("2026-11-15", [["codex", 0], ["claude", 80]], 160);

  assert.match(html, /aria-label="2026-11-15 · Claude: 80"/);
  assert.match(html, /width:50%;background:var\(--token-claude-fixed\)/);
  assert.doesNotMatch(html, /GPT\/Codex|--token-codex-fixed/);
});
