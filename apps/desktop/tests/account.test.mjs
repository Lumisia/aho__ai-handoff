import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement } from "react";
import { act, create } from "react-test-renderer";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { getAccountReport } from "../src/api.ts";
import Account from "../src/views/Account.tsx";

// Keep React, Account, its children and API/cache real. Only the native IPC
// boundary is replaced: these tests never read credentials or call a provider.
const t = (key) => key;
const originalWindow = globalThis.window;
let renderer;

afterEach(async () => {
  if (renderer) await act(async () => renderer.unmount());
  renderer = undefined;
  clearMocks();
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
});

function windowUsage(used, remaining) {
  return { used_percent: used, remaining_percent: remaining, resets_at: null,
    burn_rate_per_hour: null, projected_exhaust_at: null };
}

function slot(label, active = true) {
  return { label, email: label + "@example.test", plan: "pro", account_id: label,
    source: "oauth", created_at: null, active, path: "/test/accounts/" + label };
}

function agentReport(agent, overrides = {}) {
  return { agent, root: "/test/accounts/" + agent, active: null, plan: null,
    five_hour: null, weekly: null, usage_source: "none", slots: [], ...overrides };
}

function accountReport(agent = "claude", overrides = {}) {
  return {
    codex: agentReport("codex"),
    claude: agentReport("claude"),
    [agent]: agentReport(agent, { active: "alpha", slots: [slot("alpha")], ...overrides }),
  };
}

function slotUsage(overrides = {}) {
  return { plan: "pro", five_hour: windowUsage(37, 63), weekly: windowUsage(5, 95),
    reset_credits: null, reset_credit_details: [], ...overrides };
}

function claudeCredits(overrides = {}) {
  return slotUsage({ reset_credits: 2, claude_reset_credits: [{ remaining: 2,
    kind: "full", starts_at: null, expires_at: 4102444800 }], ...overrides });
}

function creditTitles(agent = "claude") {
  return panel(agent).findAllByProps({ className: "reset-credit-title" })
    .map((node) => node.children.join(""));
}

test("Claude saved resets show scope, count and expiry without browser actions or hints", async () => {
  await mount(accountReport(), () => claudeCredits());
  assert.deepEqual(creditTitles(), ["resetCredits 2resetCreditCountSuffix", "resetCredits 2resetCreditCountSuffix"]);
  const text = JSON.stringify(renderer.toJSON());
  assert.ok(text.includes("resetFull"));
  assert.ok(text.includes("resetExpires"));
  assert.equal(panel().findAllByType("a").length, 0);
  assert.ok(!text.includes("resetBrowserAccountHint"));
  assert.ok(!text.includes("resetOpenClaude"));
});

test("Claude unavailable inventory does not claim there are zero resets", async () => {
  await mount(accountReport(), () => slotUsage({ claude_reset_credits: null }));
  assert.ok(creditTitles().every((title) => title === "resetCredits · resetCreditsUnavailable"));
  assert.equal(creditTitles().length, 2);
});

test("Claude failed usage lookup shows unavailable without browser actions", async () => {
  await mount(accountReport(), () => Promise.reject(new Error("Provider unavailable")));
  assert.deepEqual(creditTitles(), ["resetCredits · resetCreditsUnavailable"]);
  assert.equal(panel().findAllByType("a").length, 0);
});

test("Claude known empty inventory shows zero, and expired resets are not counted", async () => {
  await mount(accountReport(), () => claudeCredits({ claude_reset_credits: [
    { remaining: 5, kind: "full", starts_at: null, expires_at: 1 },
  ] }));
  assert.deepEqual(creditTitles(), ["resetCredits 0resetCreditCountSuffix", "resetCredits 0resetCreditCountSuffix"]);
});

test("Claude reset expires while the account page remains open", async (context) => {
  context.mock.timers.enable({ apis: ["Date", "setTimeout"], now: 1794787200000 });
  await mount(accountReport(), () => claudeCredits({ claude_reset_credits: [
    { remaining: 2, kind: "scoped", starts_at: null, expires_at: 1794787230 },
  ] }));
  assert.ok(JSON.stringify(renderer.toJSON()).includes("resetScoped"));
  assert.ok(!JSON.stringify(renderer.toJSON()).includes("resetFull"));
  await act(async () => context.mock.timers.tick(30000));
  assert.deepEqual(creditTitles(), ["resetCredits 0resetCreditCountSuffix", "resetCredits 0resetCreditCountSuffix"]);
});

for (const agent of ["claude", "codex"]) {
  test(agent + ": account cards use fetched plan and hide credential source text", async () => {
    await mount(accountReport(agent, {
      plan: "pro", slots: [{ ...slot("alpha"), plan: "unknown", source: "official-cli-login" }],
    }), () => slotUsage({ plan: "pro" }));
    const card = panel(agent).findByProps({ className: "slot-main" });
    assert.equal(card.findByType("small").children.join(""), "plan: pro");
    const text = JSON.stringify(renderer.toJSON());
    assert.ok(!text.includes("unknown"));
    assert.ok(!text.includes("official-cli-login"));
  });
}

test("a forced report resolves the active account plan from its usage payload", async () => {
  await mount(accountReport("claude", { usage_source: "provider_api", active_usage: slotUsage(),
    slots: [{ ...slot("alpha"), plan: null, source: "official-cli-login" }],
  }));
  assert.equal(panel().findByProps({ className: "slot-main" }).findByType("small").children.join(""), "plan: pro");
});

test("unknown inactive plans stay absent until that account's own usage is fetched", async () => {
  await mount(accountReport("claude", { plan: "pro", slots: [slot("alpha"),
    { ...slot("beta", false), plan: "unknown", source: "official-cli-login" }],
  }), ({ label }) => slotUsage({ plan: label === "beta" ? "team" : "pro" }));
  const beta = panel().findByProps({ className: "slot-row " });
  assert.equal(beta.findByProps({ className: "slot-main" }).findAllByType("small").length, 0);
  await act(async () => { button("usageButton", beta).props.onClick(); });
  assert.equal(beta.findByProps({ className: "slot-main" }).findByType("small").children.join(""), "plan: team");
});

for (const agent of ["claude", "codex"]) {
  test(agent + ": an untagged local plan is not attributed to an unknown active account", async () => {
    await mount(accountReport(agent, { usage_source: "local_sample", plan: "team",
      slots: [{ ...slot("alpha"), plan: "unknown" }],
    }), () => Promise.reject(new Error("Provider unavailable")));
    assert.equal(panel(agent).findByProps({ className: "slot-main" }).findAllByType("small").length, 0);
    assert.equal(panel(agent).findByProps({ className: "account-panel-head" }).findAllByType("p").length, 1);
  });
}

for (const agent of ["claude", "codex"]) {
  test(agent + ": page refresh preserves the provider's full credit report without a second lookup", async () => {
    const credits = agent === "claude" ? claudeCredits() : slotUsage({ reset_credits: 2,
      reset_credit_details: [{ kind: "full", expires_at: "2099-12-11T00:00:00Z" },
        { kind: "other", expires_at: null }] });
    const state = await mount(accountReport(agent), () => credits);
    state.report = accountReport(agent, { usage_source: "provider_api", active_usage: credits });
    await refreshPage();
    assert.deepEqual(creditTitles(agent), ["resetCredits 2resetCreditCountSuffix", "resetCredits 2resetCreditCountSuffix"]);
    if (agent === "codex") {
      for (const list of panel(agent).findAllByProps({ className: "credit-list" })) {
        const [full, other] = list.findAllByType("small");
        assert.match(full.children.join(""), /^resetFull \/ resetExpires .+/);
        assert.equal(other.children.join(""), "resetOther / resetExpiryUnknown");
      }
    }
    assert.equal(state.requests.filter(({ command }) => command === "refresh_account_slot_usage").length, 1);
  });
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function mount(report, fetchUsage = () => slotUsage()) {
  globalThis.window = { setInterval, clearInterval, confirm: () => true };
  const state = { report, requests: [] };
  mockIPC((command, args) => {
    state.requests.push({ command, args });
    if (command === "get_account_report" || command === "switch_account_slot") return state.report;
    if (command === "refresh_account_slot_usage") return fetchUsage(args);
    throw new Error("Unexpected IPC command: " + command);
  });
  // Populate the real API cache with this test's initial snapshot.
  await getAccountReport({ force: true });
  await act(async () => { renderer = create(createElement(Account, { snapshot: {}, t })); });
  return state;
}

function panel(agent = "claude") {
  return renderer.root.findByProps({ className: "panel account-panel " + agent });
}

function visibleLimits(agent = "claude") {
  return panel(agent).findByProps({ className: "account-limit-stack" })
    .findAllByProps({ className: "limit-pct" }).map((node) => node.children.join(""));
}

function noSampleHints(agent = "claude") {
  return panel(agent).findAll((node) => node.type === "div" && node.children.includes("noSampleHint"));
}

function button(label, scope = renderer.root) {
  return scope.findAllByType("button").find((node) => node.children.includes(label));
}

async function refreshPage() {
  await act(async () => { button("refresh").props.onClick(); });
}

for (const agent of ["claude", "codex"]) {
  test(agent + ": successful slot usage hides the missing-sample hint even with no local sample", async () => {
    await mount(accountReport(agent));
    assert.deepEqual(visibleLimits(agent), ["63% left", "95% left"]);
    assert.equal(noSampleHints(agent).length, 0);
  });
}

for (const [name, usage, expected] of [
  ["five-hour only", slotUsage({ weekly: null }), ["63% left", "noSample"]],
  ["weekly only", slotUsage({ five_hour: null }), ["noSample", "95% left"]],
  ["zero use", slotUsage({ five_hour: windowUsage(0, 100), weekly: null }), ["100% left", "noSample"]],
]) {
  test(name + ": any available displayed window suppresses the all-missing hint", async () => {
    await mount(accountReport(), () => usage);
    assert.deepEqual(visibleLimits(), expected);
    assert.equal(noSampleHints().length, 0);
  });
}

test("no available windows retains the missing-sample hint", async () => {
  await mount(accountReport(), () => slotUsage({ five_hour: null, weekly: null }));
  assert.deepEqual(visibleLimits(), ["noSample", "noSample"]);
  assert.equal(noSampleHints().length, 1);
});

test("an empty provider response still displays the missing-sample hint", async () => {
  await mount(accountReport("claude", { usage_source: "provider_api" }),
    () => slotUsage({ five_hour: null, weekly: null }));
  assert.deepEqual(visibleLimits(), ["noSample", "noSample"]);
  assert.equal(noSampleHints().length, 1);
});

test("failed slot lookup preserves a partial local sample", async () => {
  await mount(accountReport("claude", {
    usage_source: "local_sample", weekly: windowUsage(8, 92),
  }), () => Promise.reject(new Error("Provider unavailable")));
  assert.deepEqual(visibleLimits(), ["noSample", "92% left"]);
  assert.equal(noSampleHints().length, 0);
});

test("forced refresh replaces previously fetched slot values with the new report", async () => {
  const pending = deferred();
  let calls = 0;
  const state = await mount(accountReport(), () => ++calls === 1 ? slotUsage() : pending.promise);
  assert.deepEqual(visibleLimits(), ["63% left", "95% left"]);
  state.report = accountReport("claude", {
    usage_source: "provider_api", five_hour: windowUsage(60, 40), weekly: windowUsage(10, 90),
  });
  await refreshPage();
  assert.deepEqual(visibleLimits(), ["40% left", "90% left"]);
  assert.equal(noSampleHints().length, 0);
  assert.equal(state.requests.filter(({ command, args }) => command === "get_account_report" && args.force).length, 2);
});

test("an automatic slot response started before refresh cannot overwrite its newer report", async () => {
  const oldResponse = deferred();
  const state = await mount(accountReport(), () => oldResponse.promise);
  state.report = accountReport("claude", {
    usage_source: "provider_api", five_hour: windowUsage(60, 40), weekly: windowUsage(10, 90),
  });
  await refreshPage();
  await act(async () => oldResponse.resolve(slotUsage()));
  assert.deepEqual(visibleLimits(), ["40% left", "90% left"]);
});

test("a manual slot response from an old report cannot replace the new account's usage", async () => {
  const oldResponse = deferred();
  let calls = 0;
  const state = await mount(accountReport(), () => ++calls === 1 ? slotUsage() : oldResponse.promise);
  await act(async () => { button("usageButton", panel()).props.onClick(); });
  state.report = accountReport("claude", {
    usage_source: "provider_api", five_hour: windowUsage(60, 40), weekly: windowUsage(10, 90),
    // A refreshed/recreated slot may reuse a label with different credentials.
    slots: [{ ...slot("alpha"), email: "new@example.test", account_id: "new-account" }],
  });
  await refreshPage();
  await act(async () => oldResponse.resolve(slotUsage()));
  assert.deepEqual(visibleLimits(), ["40% left", "90% left"]);
  assert.equal(button("usageButton", panel()).props.disabled, false);
});

test("switching away and back retries a cancelled automatic lookup", async () => {
  const oldResponse = deferred();
  let alphaCalls = 0;
  const state = await mount(accountReport("claude", { slots: [slot("alpha"), slot("beta", false)] }),
    ({ label }) => label === "alpha" && ++alphaCalls === 1 ? oldResponse.promise : slotUsage());
  state.report = accountReport("claude", {
    active: "beta", slots: [slot("alpha", false), slot("beta")],
  });
  await act(async () => {
    panel().findAllByType("button").find((node) => node.children.includes("switch") && !node.props.disabled).props.onClick();
  });
  state.report = accountReport("claude", { slots: [slot("alpha"), slot("beta", false)] });
  await act(async () => {
    panel().findAllByType("button").find((node) => node.children.includes("switch") && !node.props.disabled).props.onClick();
  });
  assert.deepEqual(visibleLimits(), ["63% left", "95% left"]);
  assert.equal(noSampleHints().length, 0);
  assert.equal(button("usageButton", panel()).props.disabled, false);
  await act(async () => oldResponse.resolve(slotUsage({ five_hour: windowUsage(90, 10) })));
  assert.deepEqual(visibleLimits(), ["63% left", "95% left"]);
});

test("a page refresh started before switching cannot restore the previous active account", async () => {
  const alpha = accountReport("claude", { slots: [slot("alpha"), slot("beta", false)] });
  const state = await mount(alpha);
  const previous = deferred();
  state.report = previous.promise;
  await refreshPage();
  state.report = accountReport("claude", {
    active: "beta", slots: [slot("alpha", false), slot("beta")],
  });
  await act(async () => {
    panel().findAllByType("button").find((node) => node.children.includes("switch") && !node.props.disabled).props.onClick();
  });
  await act(async () => previous.resolve(alpha));
  const active = panel().findByProps({ className: "slot-row active" });
  assert.equal(active.findByProps({ className: "slot-main" }).findByType("strong").children.join(""), "beta@example.test");
});
