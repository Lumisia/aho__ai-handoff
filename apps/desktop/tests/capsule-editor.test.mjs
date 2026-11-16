import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement } from "react";
import { act, create } from "react-test-renderer";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createTranslator } from "../src/i18n.ts";
import Capsules from "../src/views/Capsules.tsx";

const originalWindow = globalThis.window;
const originalDocument = globalThis.document;
let renderer;
let listeners;
const item = {
  capsule_id: "cap_demo", project_id: "demo", project_label: "Demo",
  created_at: "2026-11-16T00:00:00Z", source_agent: "codex", target_agent: "any",
  state: "pending", summary_preview: "Original goal", path: "/store/capsules/demo/cap_demo.json",
};
const original = JSON.stringify({ capsule_id: "cap_demo", source_agent: "codex", target_agent: null, summary: { goal: "Original goal" } });

afterEach(async () => {
  if (renderer) await act(async () => renderer.unmount());
  renderer = undefined;
  clearMocks();
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
  if (originalDocument === undefined) delete globalThis.document;
  else globalThis.document = originalDocument;
});

async function mount(handle = () => undefined) {
  globalThis.window = {};
  listeners = new Map();
  globalThis.document = {
    addEventListener: (type, handler) => listeners.set(type, handler),
    removeEventListener: (type, handler) => { if (listeners.get(type) === handler) listeners.delete(type); },
  };
  const state = { requests: [], changed: 0 };
  mockIPC((command, args) => {
    state.requests.push({ command, args });
    const result = handle(command, args);
    if (result !== undefined) return result;
    if (command === "read_capsule") return { path: args.path, text: original, truncated: false };
    if (command === "read_capsule_editor") return original;
    if (command === "save_capsule") return null;
    throw new Error(`Unexpected IPC ${command}`);
  });
  const props = {
    initial: { items: [item], pending_count: 1, skipped: 0 }, selectedPath: item.path,
    onSelectedPathChange: () => {}, onChanged: () => { state.changed++; },
    onDeleteCapsule: () => {}, onCopyPath: () => {}, onOpenFolder: () => {}, onOpenWith: () => {},
    t: createTranslator("ko"),
  };
  await act(async () => {
    renderer = create(createElement(Capsules, props), { createNodeMock: () => ({ contains: (target) => target === "inside" }) });
  });
  return { state, props };
}

function text(node) {
  if (typeof node === "string") return node;
  if (node.props?.["aria-hidden"] === true || node.props?.["aria-hidden"] === "true") return "";
  return (node.children ?? []).map(text).join("");
}
function button(label) { return renderer.root.findAllByType("button").find((node) => text(node) === label); }
async function edit() { await act(async () => { button("편집").props.onClick(); }); }

test("the complete capsule body is editable and saves without changing an untouched any target", async () => {
  const { state } = await mount();
  await edit();
  const textarea = renderer.root.findByType("textarea");
  assert.equal(textarea.props.value, original);
  assert.equal(renderer.root.findAllByType("select").length, 0);
  const draft = original.replace("Original goal", "Edited goal");
  await act(async () => { textarea.props.onChange({ target: { value: draft } }); });
  await act(async () => { button("저장").props.onClick(); });
  const save = state.requests.find(({ command }) => command === "save_capsule");
  assert.equal(save.args.content, draft);
  assert.equal(save.args.originalContent, original);
  assert.equal(save.args.sourceAgent, undefined);
  assert.equal(save.args.targetAgent, undefined);
  assert.equal(state.changed, 1);
});

test("route dropdowns show the two branded agents and send the canonical Claude identity", async () => {
  const { state } = await mount();
  await edit();
  await act(async () => { renderer.root.findByProps({ "aria-label": "대상 에이전트" }).props.onClick(); });
  const menu = renderer.root.findByProps({ className: "capsule-state-menu capsule-agent-menu" });
  assert.deepEqual(menu.findAllByType("button").map(text), ["코덱스", "클로드"]);
  assert.equal(menu.findAll((node) => node.props.className === "agent-logo codex").length, 1);
  assert.equal(menu.findAll((node) => node.props.className === "agent-logo claude").length, 1);
  await act(async () => { menu.findAllByType("button")[1].props.onClick(); });
  await act(async () => { button("저장").props.onClick(); });
  assert.equal(state.requests.find(({ command }) => command === "save_capsule").args.targetAgent, "claude-code");
});

test("cancel discards edits and a save error preserves the draft", async () => {
  const { state } = await mount((command) => command === "save_capsule" ? Promise.reject("Capsule changed") : undefined);
  await edit();
  await act(async () => { renderer.root.findByType("textarea").props.onChange({ target: { value: "edited draft" } }); });
  await act(async () => { button("저장").props.onClick(); });
  assert.equal(renderer.root.findByType("textarea").props.value, "edited draft");
  assert.ok(renderer.root.findByProps({ className: "inline-error" }).children.includes("Capsule changed"));
  await act(async () => { button("취소").props.onClick(); });
  assert.equal(renderer.root.findAllByType("textarea").length, 0);
  assert.equal(state.changed, 0);
});

test("detail menus dismiss on outside pointer input and Escape", async () => {
  await mount();
  await act(async () => { renderer.root.findByProps({ title: "더보기" }).props.onClick(); });
  assert.equal(renderer.root.findAllByProps({ className: "capsule-detail-menu" }).length, 1);
  await act(async () => { listeners.get("pointerdown")?.({ target: "outside" }); });
  assert.equal(renderer.root.findAllByProps({ className: "capsule-detail-menu" }).length, 0);
  await act(async () => { renderer.root.findByProps({ title: "더보기" }).props.onClick(); });
  await act(async () => { listeners.get("keydown")?.({ key: "Escape" }); });
  assert.equal(renderer.root.findAllByProps({ className: "capsule-detail-menu" }).length, 0);
});

test("a delayed editor load cannot open an editor on a different capsule", async () => {
  let resolveEditor;
  const { props } = await mount((command) => command === "read_capsule_editor" ? new Promise((resolve) => { resolveEditor = resolve; }) : undefined);
  await edit();
  const other = { ...item, capsule_id: "cap_other", path: "/store/capsules/demo/cap_other.json" };
  await act(async () => { renderer.update(createElement(Capsules, { ...props, initial: { ...props.initial, items: [other] }, selectedPath: other.path })); });
  await act(async () => { resolveEditor(original); });
  assert.equal(renderer.root.findAllByType("textarea").length, 0);
});

test("state changes stay disabled while editing and route menus dismiss outside", async () => {
  await mount();
  await edit();
  assert.equal(renderer.root.findByProps({ title: "상태 변경" }).props.disabled, true);
  await act(async () => { renderer.root.findByProps({ "aria-label": "출발 에이전트" }).props.onClick(); });
  await act(async () => { listeners.get("pointerdown")({ target: "inside" }); });
  assert.equal(renderer.root.findAllByProps({ className: "capsule-state-menu capsule-agent-menu" }).length, 1);
  await act(async () => { listeners.get("pointerdown")({ target: "outside" }); });
  assert.equal(renderer.root.findAllByProps({ className: "capsule-state-menu capsule-agent-menu" }).length, 0);
});

test("truncated capsule previews cannot be edited", async () => {
  await mount((command, args) => command === "read_capsule" ? { path: args.path, text: original, truncated: true } : undefined);
  assert.equal(button("편집").props.disabled, true);
});

test("repeated save clicks submit one write and a delayed result cannot replace another capsule", async () => {
  let resolveSave;
  const { state, props } = await mount((command) => command === "save_capsule" ? new Promise((resolve) => { resolveSave = resolve; }) : undefined);
  await edit();
  await act(async () => {
    const save = button("저장");
    save.props.onClick();
    save.props.onClick();
  });
  assert.equal(state.requests.filter(({ command }) => command === "save_capsule").length, 1);
  const other = { ...item, capsule_id: "cap_other", path: "/store/capsules/demo/cap_other.json" };
  await act(async () => { renderer.update(createElement(Capsules, { ...props, initial: { ...props.initial, items: [other] }, selectedPath: other.path })); });
  await act(async () => { resolveSave(null); });
  assert.equal(renderer.root.findAllByType("textarea").length, 0);
  assert.equal(button("편집").props.disabled, false);
});

test("a failed post-save preview can retry without restoring the old body or switching capsules", async () => {
  let reads = 0;
  const committed = original.replace("Original goal", "Committed goal");
  const { state } = await mount((command, args) => {
    if (command !== "read_capsule") return undefined;
    reads++;
    if (reads === 2) return Promise.reject("Preview temporarily unavailable");
    return { path: args.path, text: reads > 2 ? committed : original, truncated: false };
  });
  await edit();
  await act(async () => { renderer.root.findByType("textarea").props.onChange({ target: { value: committed } }); });
  await act(async () => { button("저장").props.onClick(); });
  assert.equal(state.requests.filter(({ command }) => command === "save_capsule").length, 1);
  assert.equal(button("편집").props.disabled, true);
  assert.notEqual(text(renderer.root.findByType("pre")), original);
  assert.ok(button("새로고침"), "failed preview offers a retry");
  await act(async () => { button("새로고침").props.onClick(); });
  assert.equal(text(renderer.root.findByType("pre")), committed);
  assert.equal(button("편집").props.disabled, false);
  assert.equal(state.requests.filter(({ command }) => command === "save_capsule").length, 1);
});

test("a stale editor preview can refresh and reopen the same capsule", async () => {
  const latest = original.replace("Original goal", "External edit");
  let reads = 0;
  const { state } = await mount((command, args) => {
    if (command === "read_capsule") {
      return { path: args.path, text: reads++ === 0 ? original : latest, truncated: false };
    }
    if (command === "read_capsule_editor") {
      return args.originalContent === original
        ? Promise.reject("The capsule changed. Reopen it before editing again.")
        : latest;
    }
    return undefined;
  });
  await edit();
  assert.equal(renderer.root.findAllByType("textarea").length, 0);
  assert.ok(button("새로고침"), "stale preview offers a retry while the old text is still visible");
  await act(async () => { button("새로고침").props.onClick(); });
  assert.equal(text(renderer.root.findByType("pre")), latest);
  await edit();
  assert.equal(renderer.root.findByType("textarea").props.value, latest);
  assert.equal(state.requests.filter(({ command }) => command === "save_capsule").length, 0);
});
