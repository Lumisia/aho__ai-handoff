import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { getAccountReport, switchAccountSlot } from "../src/api.ts";

const originalWindow = globalThis.window;
afterEach(() => {
  clearMocks();
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
});

test("a report started before switching cannot replace the switched account cache", async () => {
  globalThis.window = {};
  let finishOld;
  const switched = { claude: { active: "beta" } };
  mockIPC((command) => {
    if (command === "get_account_report") return new Promise((resolve) => { finishOld = resolve; });
    if (command === "switch_account_slot") return switched;
    throw new Error(`Unexpected IPC command ${command}`);
  });
  const old = getAccountReport({ force: true });
  await switchAccountSlot("claude", "beta");
  finishOld({ claude: { active: "alpha" } });
  await old;
  assert.equal(await getAccountReport(), switched);
});

test("a slower forced account refresh cannot overwrite a newer forced result", async () => {
  globalThis.window = {};
  const pending = [];
  mockIPC(() => new Promise((resolve) => pending.push(resolve)));
  const old = getAccountReport({ force: true });
  const current = getAccountReport({ force: true });
  const fresh = { claude: { active: "new" } };
  pending[1](fresh);
  await current;
  pending[0]({ claude: { active: "old" } });
  await old;
  assert.equal(await getAccountReport(), fresh);
});
