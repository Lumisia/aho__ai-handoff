import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { act, create } from "react-test-renderer";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Overview from "../src/views/Overview.tsx";

test("Overview explains when Codex weekly usage is projected to last until reset", async () => {
  const resetsAt = 1795105500;
  const window = { used_percent: 50, remaining_percent: 50, resets_at: resetsAt, window_minutes: 10080 };
  const previousWindow = globalThis.window;
  globalThis.window = {};
  mockIPC((command) => {
    if (command === "get_account_report") return {
      claude: { active: null },
      codex: { active: "test-account", five_hour: { ...window, projected_exhaust_at: resetsAt - 3600 },
        weekly: { ...window, projected_exhaust_at: resetsAt + 21600 } },
    };
    if (command === "get_usage_report") return null;
    throw new Error(`Unexpected IPC command ${command}`);
  });
  const check = { id: "test", label: "test", status: "ok" };
  const snapshot = Object.fromEntries(["codex_hooks", "codex_config", "claude_settings", "ipc", "store"]
    .map((key) => [key, { ...check, id: key }]));
  let renderer;
  try {
    await act(async () => { renderer = create(createElement(Overview, { snapshot, t: (key) => key })); });
    const [session, weekly] = renderer.root.findAllByProps({ className: "limit-row codex" });
    assert.ok(session.findByProps({ className: "limit-exhaust" }).children.join("").startsWith("runsOutAt"));
    assert.ok(weekly.findAllByType("small").some((node) => node.children.includes("lastsUntilReset")));
  } finally {
    if (renderer) await act(async () => renderer.unmount());
    clearMocks();
    if (previousWindow === undefined) delete globalThis.window;
    else globalThis.window = previousWindow;
  }
});
