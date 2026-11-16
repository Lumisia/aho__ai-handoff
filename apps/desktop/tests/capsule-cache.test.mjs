import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { deleteCapsule, deleteCapsuleProject, getDashboardSnapshot, saveCapsule, setCapsuleField, setCapsuleState, toggleCapsuleState } from "../src/api.ts";

const originalWindow = globalThis.window;
afterEach(() => {
  clearMocks();
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
});

test("an old dashboard request cannot restore capsules after project deletion", async () => {
  globalThis.window = {};
  const pending = [];
  mockIPC((command) => {
    if (command === "get_dashboard_snapshot") {
      return new Promise((resolve) => pending.push(resolve));
    }
    if (command === "delete_capsule_project") return;
    throw new Error(`Unexpected IPC command ${command}`);
  });
  const stale = getDashboardSnapshot({ force: true });
  await deleteCapsuleProject("demo", "confirmed-revision");
  const refreshed = getDashboardSnapshot({ force: true });
  assert.equal(pending.length, 2);
  const empty = { capsules: { items: [] } };
  pending[1](empty);
  assert.equal(await refreshed, empty);
  pending[0]({ capsules: { items: [{ capsule_id: "deleted" }] } });
  await stale;
  assert.equal(await getDashboardSnapshot(), empty);
});

test("saving the body forwards canonical route IDs and invalidates the dashboard", async () => {
  globalThis.window = {};
  const requests = [];
  mockIPC((command, args) => {
    requests.push({ command, args });
    if (command === "get_dashboard_snapshot") return { version: requests.length };
    if (command === "save_capsule") return;
    throw new Error(`Unexpected IPC command ${command}`);
  });
  await getDashboardSnapshot({ force: true });
  await saveCapsule("/fake/capsule.json", "draft", "original", { target: "claude-code" });
  await getDashboardSnapshot();
  assert.deepEqual(requests.map(({ command }) => command), [
    "get_dashboard_snapshot", "save_capsule", "get_dashboard_snapshot",
  ]);
  assert.deepEqual(requests[1].args, {
    path: "/fake/capsule.json", content: "draft", originalContent: "original",
    sourceAgent: undefined, targetAgent: "claude-code",
  });
});

for (const [command, mutate] of [
  ["delete_capsule", () => deleteCapsule("/fake/capsule.json")],
  ["set_capsule_state", () => setCapsuleState("/fake/capsule.json", "archived")],
  ["set_capsule_field", () => setCapsuleField("/fake/capsule.json", "goal", "Changed")],
  ["toggle_capsule_state", () => toggleCapsuleState("/fake/capsule.json")],
]) {
  test(`${command} cannot reuse a dashboard request started before the mutation`, async () => {
    globalThis.window = {};
    const pending = [];
    mockIPC((name) => {
      if (name === "get_dashboard_snapshot") return new Promise((resolve) => pending.push(resolve));
      if (name === command) return "archived";
      throw new Error(`Unexpected IPC command ${name}`);
    });
    const stale = getDashboardSnapshot({ force: true });
    await mutate();
    const refreshed = getDashboardSnapshot({ force: true });
    // Settle both requests even on failure, so no timeout outlives this test.
    const requestCount = pending.length;
    pending.forEach((resolve, index) => resolve({ revision: index }));
    await Promise.all([stale, refreshed]);
    assert.equal(requestCount, 2);
    assert.equal((await getDashboardSnapshot()).revision, 1);
  });
}
