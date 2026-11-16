import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement } from "react";
import { act, create } from "react-test-renderer";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { getConfigSettings } from "../src/api.ts";
import Settings from "../src/views/Settings.tsx";

const t = (key) => key;
const originalWindow = globalThis.window;
const originalDocument = globalThis.document;
const originalSelf = globalThis.self;
let renderer;

afterEach(async () => {
  if (renderer) await act(async () => renderer.unmount());
  renderer = undefined;
  clearMocks();
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
  if (originalDocument === undefined) delete globalThis.document;
  else globalThis.document = originalDocument;
  if (originalSelf === undefined) delete globalThis.self;
  else globalThis.self = originalSelf;
});

// The components and picker stay real. Native settings writes and browser
// layout/events are the boundaries replaced here; user settings are untouched.
async function mount({ width = 1040, height = 640,
  anchor = { left: 938, right: 1038, top: 520, bottom: 550 },
  popup = { width: 300, height: 346 } } = {}) {
  const state = { anchor, popup, requests: [], listeners: new Map() };
  const browser = {
    innerWidth: width, innerHeight: height, pageXOffset: 0, pageYOffset: 0,
    addEventListener(type, listener) {
      if (!state.listeners.has(type)) state.listeners.set(type, new Set());
      state.listeners.get(type).add(listener);
    },
    removeEventListener(type, listener) { state.listeners.get(type)?.delete(listener); },
  };
  const document = { defaultView: browser,
    createElement: () => ({ setAttribute() {} }), head: { appendChild() {} } };
  globalThis.window = browser;
  globalThis.document = document;
  globalThis.self = browser;
  state.fire = async (type, values) => {
    const event = { preventDefault() { this.prevented = true; },
      stopPropagation() { this.stopped = true; }, ...values };
    await act(async () => {
      for (const listener of [...state.listeners.get(type) ?? []]) listener(event);
    });
    return event;
  };

  const row = { key: "gui_theme.focus_border_color", value: "#2F6F50", default_value: "#2F6F50",
    kind: "color", category: "theme", description: "Focus color" };
  let rows = [row];
  mockIPC((command, args) => {
    state.requests.push({ command, args });
    if (command === "get_config_settings") return rows;
    if (command === "get_theme") return { mode: "light", catalog: [], light_theme: "white", dark_theme: "dark" };
    if (command === "set_config_value") {
      rows = [{ ...row, value: args.value }];
      return rows;
    }
    if (command === "reset_config_value") { rows = [row]; return rows; }
    throw new Error("Unexpected IPC command: " + command);
  });
  await getConfigSettings({ force: true });
  await act(async () => {
    renderer = create(createElement(Settings, { t }), { createNodeMock(element) {
      const className = element.props.className ?? "";
      const scope = className.includes("setting-popover") ? "popup"
        : className.includes("setting-value-button") ? "trigger" : className;
      const node = { ownerDocument: document,
        getBoundingClientRect: () => scope === "trigger" ? state.anchor : state.popup,
        get scrollHeight() { return state.popup.height - 2; },
        get offsetHeight() { return state.popup.height; },
        get clientHeight() { return state.popup.height - 2; },
        contains: (target) => target === node || target?.scope === scope,
        focus() { state.focused = node; } };
      if (scope === "trigger") state.trigger = node;
      return node;
    } });
  });
  await act(async () => {
    renderer.root.findAllByType("button").find((node) =>
      node.props.className?.includes("category") &&
      node.findAllByType("span").some((span) => span.children.includes("theme"))).props.onClick();
  });
  await act(async () => renderer.root.findByProps({ className: "setting-value-button" }).props.onClick());
  return state;
}

function popovers() {
  return renderer.root.findAll((node) => node.type === "div" &&
    node.props.className?.split(" ").includes("setting-popover"));
}

test("the color popup fits its actual height above a low trigger and stays inside the right edge", async () => {
  await mount();
  const style = popovers()[0].props.style;
  assert.equal(style?.top, 168);
  assert.equal(style?.left, 732);
  assert.equal(style?.maxHeight, 624);
});

test("a high trigger opens below and clamps the popup to the left edge", async () => {
  await mount({ anchor: { left: 5, right: 105, top: 100, bottom: 130 } });
  assert.equal(popovers()[0].props.style?.top, 136);
  assert.equal(popovers()[0].props.style?.left, 8);
});

test("a smaller viewport keeps the popup accessible with a bounded scroll area", async () => {
  await mount({ width: 280, height: 260, anchor: { left: 100, right: 200, top: 120, bottom: 150 } });
  const style = popovers()[0].props.style;
  assert.equal(style?.top, 8);
  assert.equal(style?.left, 8);
  assert.equal(style?.maxHeight, 244);
});

test("scroll and resize reposition the popup using the trigger's new viewport coordinates", async () => {
  const state = await mount();
  state.anchor = { left: 5, right: 105, top: 40, bottom: 70 };
  await state.fire("scroll", { target: { scope: "settings" } });
  assert.equal(popovers()[0].props.style?.top, 76);
  globalThis.window.innerHeight = 300;
  await state.fire("resize", {});
  assert.equal(popovers()[0].props.style?.top, 8);
  assert.equal(popovers()[0].props.style?.maxHeight, 284);
});

test("inside picker and trigger clicks keep the popup open, while outside click dismisses without writing", async () => {
  const state = await mount();
  await state.fire("pointerdown", { target: { scope: "popup" } });
  assert.equal(popovers().length, 1);
  await state.fire("pointerdown", { target: { scope: "trigger" } });
  assert.equal(popovers().length, 1);
  await state.fire("pointerdown", { target: { scope: "other-setting" } });
  assert.equal(popovers().length, 0);
  assert.equal(state.requests.filter(({ command }) => command === "set_config_value").length, 0);
  assert.equal(state.listeners.get("pointerdown")?.size, 0);
});

test("Escape dismisses only the popup, restores trigger focus, and removes its handlers", async () => {
  const state = await mount();
  const event = await state.fire("keydown", { key: "Escape" });
  assert.equal(popovers().length, 0);
  assert.equal(event.prevented, true);
  assert.equal(event.stopped, true);
  assert.equal(state.focused, state.trigger);
  for (const type of ["pointerdown", "keydown", "scroll", "resize"]) {
    assert.equal(state.listeners.get(type)?.size, 0, type);
  }
});

test("the real hue picker still edits the draft without dismissing or saving it", async () => {
  const state = await mount();
  await state.fire("pointerdown", { target: { scope: "popup" } });
  const event = await state.fire("keydown", { key: "ArrowRight", which: 39 });
  const hue = popovers()[0].find((node) => node.type === "div" && node.props["aria-label"] === "Hue");
  await act(async () => hue.props.onKeyDown(event));
  assert.equal(popovers().length, 1);
  const draft = popovers()[0].findAllByType("input").find((node) => node.props.type !== "color").props.value;
  assert.notEqual(draft.toLowerCase(), "#2f6f50");
  assert.equal(state.requests.filter(({ command }) => command === "set_config_value").length, 0);
  await act(async () => popovers()[0].findByProps({ className: "full-width" }).props.onClick());
  assert.equal(state.requests.find(({ command }) => command === "set_config_value").args.value, draft);
});

test("editing then applying keeps the existing settings commit and reset behavior", async () => {
  const state = await mount();
  const text = popovers()[0].findAllByType("input").find((node) => node.props.type !== "color");
  await act(async () => text.props.onChange({ target: { value: "#123456" } }));
  await state.fire("pointerdown", { target: { scope: "popup" } });
  assert.equal(popovers().length, 1);
  await act(async () => popovers()[0].findByProps({ className: "full-width" }).props.onClick());
  assert.equal(popovers().length, 0);
  assert.equal(renderer.root.findByProps({ className: "setting-value-button" }).findByType("code").children[0], "#123456");
  assert.deepEqual(state.requests.find(({ command }) => command === "set_config_value").args,
    { key: "gui_theme.focus_border_color", value: "#123456" });
  await act(async () => renderer.root.findByProps({ className: "reset-icon" }).props.onClick());
  assert.equal(renderer.root.findByProps({ className: "setting-value-button" }).findByType("code").children[0], "#2F6F50");
  assert.deepEqual(state.requests.find(({ command }) => command === "reset_config_value").args,
    { key: "gui_theme.focus_border_color" });
});
