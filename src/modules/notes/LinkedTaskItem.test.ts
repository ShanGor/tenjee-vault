import { expect, it } from "vitest";
import { Schema } from "@tiptap/pm/model";
import { EditorState } from "@tiptap/pm/state";
import { identifyTaskItems } from "./LinkedTaskItem";

const schema = new Schema({ nodes: {
  doc: { content: "taskItem+" },
  text: {},
  taskItem: { content: "text*", attrs: { checked: { default: false }, nodeId: { default: null }, taskId: { default: null } } },
} });

it("upgrades old items without losing text or state, preserving unique existing links", () => {
  const doc = schema.nodeFromJSON({ type: "doc", content: [
    { type: "taskItem", attrs: { checked: true }, content: [{ type: "text", text: "Legacy item" }] },
    { type: "taskItem", attrs: { nodeId: "existing", taskId: "task" }, content: [{ type: "text", text: "Original" }] },
    { type: "taskItem", attrs: { nodeId: "existing", taskId: "task" }, content: [{ type: "text", text: "Copy" }] },
  ] });
  const state = EditorState.create({ doc });
  const transaction = state.tr;
  expect(identifyTaskItems(doc, transaction)).toBe(true);
  const next = state.apply(transaction);
  expect(next.doc.textContent).toBe(doc.textContent);
  expect(next.doc.child(0).attrs.checked).toBe(true);
  expect(next.doc.child(1).attrs).toMatchObject({ nodeId: "existing", taskId: "task" });
  expect(next.doc.child(2).attrs.taskId).toBeNull();
  expect(new Set([0, 1, 2].map((index) => next.doc.child(index).attrs.nodeId)).size).toBe(3);
  expect(identifyTaskItems(next.doc, next.tr)).toBe(false);
  expect(schema.nodeFromJSON(next.doc.toJSON()).eq(next.doc)).toBe(true);
});
