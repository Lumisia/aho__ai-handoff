import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement } from "react";
import { act, create } from "react-test-renderer";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import ProjectDeleteDialog from "../src/components/ProjectDeleteDialog.tsx";
import { createTranslator } from "../src/i18n.ts";

const originalWindow = globalThis.window;
let renderer;

afterEach(async () => {
  if (renderer) await act(async () => renderer.unmount());
  renderer = undefined;
  clearMocks();
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
});

async function mount(deleteResult = () => undefined) {
  globalThis.window = {};
  const state = { requests: [], closed: 0, deleted: 0 };
  mockIPC((command, args) => {
    state.requests.push({ command, args });
    if (command === "get_capsule_project") {
      return { path: "/test/store/capsules/demo", capsule_count: 5, revision: "test-inventory" };
    }
    if (command === "delete_capsule_project") return deleteResult();
    throw new Error(`Unexpected IPC command ${command}`);
  });
  await act(async () => {
    renderer = create(createElement(ProjectDeleteDialog, {
      projectId: "demo",
      projectLabel: "Demo project",
      onClose: () => { state.closed++; },
      onDeleted: () => { state.deleted++; },
      t: createTranslator("ko"),
    }));
  });
  return state;
}

function button(label) {
  return renderer.root.findAllByType("button").find((item) => item.children.includes(label));
}

test("project deletion presents the whole folder count and requires a confirmation", async () => {
  const state = await mount();
  const dialog = renderer.root.findByProps({ role: "alertdialog" });
  assert.ok(dialog.findByProps({ id: "project-delete-count" }).children.includes("5개의 캡슐도 함께 삭제됩니다."));
  await act(async () => { button("취소").props.onClick(); });
  assert.equal(state.closed, 1);
  assert.deepEqual(state.requests.map(({ command }) => command), ["get_capsule_project"]);
});

test("confirmation sends the exact project revision once", async () => {
  const state = await mount();
  await act(async () => {
    const confirm = button("프로젝트 삭제");
    confirm.props.onClick();
    confirm.props.onClick();
  });
  assert.deepEqual(state.requests.filter(({ command }) => command === "delete_capsule_project"), [{
    command: "delete_capsule_project",
    args: { projectId: "demo", expectedRevision: "test-inventory" },
  }]);
  assert.equal(state.deleted, 1);
});

test("a changed project remains intact and needs a new confirmation", async () => {
  const state = await mount(() => Promise.reject("The project changed. Reopen confirmation."));
  await act(async () => { button("프로젝트 삭제").props.onClick(); });
  assert.equal(state.deleted, 0);
  assert.ok(renderer.root.findByProps({ role: "alert" }).children.includes("The project changed. Reopen confirmation."));
  assert.equal(button("프로젝트 삭제").props.disabled, true);
});
