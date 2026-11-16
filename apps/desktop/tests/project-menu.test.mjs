import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement } from "react";
import { act, create } from "react-test-renderer";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import App from "../src/App.tsx";

const originalWindow = globalThis.window;
const originalDocument = globalThis.document;
let renderer;

afterEach(async () => {
  if (renderer) await act(async () => renderer.unmount());
  renderer = undefined;
  clearMocks();
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
  if (originalDocument === undefined) delete globalThis.document;
  else globalThis.document = originalDocument;
});

const check = { id: "test", label: "Test", status: "ok", message: "Ready", path: null };
const capsules = [
  { capsule_id: "cap_codex", source_agent: "codex", state: "pending" },
  { capsule_id: "cap_claude", source_agent: "claude-code", state: "consumed" },
].map((item) => ({
  ...item,
  project_id: "demo", project_label: "Demo project", created_at: "2026-11-16T09:00:00Z",
  target_agent: "any", summary_preview: item.capsule_id,
  path: `/test/store/capsules/demo/${item.capsule_id}.json`,
}));

async function mount() {
  const listeners = new Map();
  globalThis.window = {
    innerWidth: 1000, innerHeight: 800,
    setInterval: () => 1, clearInterval: () => {},
    addEventListener: (name, callback) => listeners.set(name, callback),
    removeEventListener: (name) => listeners.delete(name),
  };
  globalThis.document = { addEventListener() {}, removeEventListener() {} };
  mockWindows("main");
  const requests = [];
  mockIPC((command, args) => {
    requests.push({ command, args });
    if (command === "get_dashboard_snapshot") return {
      paths: {}, install_state: {}, daemon: check, autostart: check,
      codex_hooks: { ...check, id: "codex-hooks" },
      codex_config: { ...check, id: "codex-config" },
      claude_settings: { ...check, id: "claude-settings" },
      ipc: { ...check, id: "ipc" }, store: { ...check, id: "store" },
      duplicates: [], checks: [], capsules: { items: capsules, pending_count: 1, skipped: 0 },
    };
    if (command === "get_theme") return { language: "ko", mode: "dark", dark_theme: "custom", catalog: [] };
    if (command === "get_app_version") return "test";
    if (command === "check_app_update") return { current: "test", update_available: false };
    if (command === "get_limit_alerts") return [];
    if (command === "get_account_report") return { codex: { active: null }, claude: { active: null } };
    if (command === "get_usage_report") return Promise.reject("No usage in this fixture");
    if (command === "get_capsule_project") return {
      path: "/test/store/capsules/demo", capsule_count: 2, revision: "whole-project-revision",
    };
    throw new Error(`Unexpected IPC command ${command}`);
  });
  await act(async () => { renderer = create(createElement(App)); });
  return { requests, listeners };
}

function text(node) {
  return node.children.map((child) => typeof child === "string" ? child : text(child)).join("");
}

function projectButtons() {
  return renderer.root.findAllByType("button").filter((node) => node.props.className?.startsWith("side-project-button"));
}

async function openProjectMenu() {
  let prevented = false;
  await act(async () => projectButtons()[0].props.onContextMenu({
    clientX: 120, clientY: 200, preventDefault: () => { prevented = true; },
  }));
  assert.equal(prevented, true, "the browser context menu is prevented");
  return renderer.root.findByProps({ className: "capsule-context-menu" });
}

test("project right-click uses the AHO menu and outside click dismisses it", async () => {
  await mount();
  const menu = await openProjectMenu();
  assert.deepEqual(menu.findAllByType("button").map(text), ["프로젝트 삭제", "경로 복사", "폴더 열기"]);
  await act(async () => renderer.root.findByProps({ className: "context-scrim" }).props.onMouseDown());
  assert.equal(renderer.root.findAllByProps({ className: "capsule-context-menu" }).length, 0);
});

test("filtered project deletion confirms all agents' capsules and cancel never deletes", async () => {
  const { requests } = await mount();
  const filter = renderer.root.findByProps({ className: "sidebar-capsule-filter" }).findByType("select");
  await act(async () => filter.props.onChange({ target: { value: "pending" } }));
  assert.equal(projectButtons().length, 1, "the Claude-only consumed entry is hidden by the filter");
  const menu = await openProjectMenu();
  await act(async () => menu.findAllByType("button")[0].props.onClick());
  assert.equal(renderer.root.findAllByProps({ className: "capsule-context-menu" }).length, 0);
  const dialog = renderer.root.findByProps({ role: "alertdialog" });
  assert.equal(text(dialog.findByProps({ id: "project-delete-count" })), "2개의 캡슐도 함께 삭제됩니다.");
  assert.deepEqual(requests.filter(({ command }) => command === "get_capsule_project"), [{
    command: "get_capsule_project", args: { projectId: "demo" },
  }]);
  await act(async () => dialog.findAllByType("button").find((node) => text(node) === "취소").props.onClick());
  assert.equal(renderer.root.findAllByProps({ role: "alertdialog" }).length, 0);
  assert.equal(requests.some(({ command }) => command === "delete_capsule_project"), false);
});
