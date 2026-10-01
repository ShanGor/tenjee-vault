import TaskItem from "@tiptap/extension-task-item";
import { Plugin } from "@tiptap/pm/state";
import type { Node } from "@tiptap/pm/model";
import type { Transaction } from "@tiptap/pm/state";

/** Upgrade legacy items and detach copied items from the original task. */
export function identifyTaskItems(doc: Node, transaction: Transaction): boolean {
  const seen = new Set<string>();
  let changed = false;
  doc.descendants((node, pos) => {
    if (node.type.name !== "taskItem") return;
    const id = node.attrs.nodeId as string | null;
    if (!id || seen.has(id)) {
      const nodeId = crypto.randomUUID();
      transaction.setNodeMarkup(pos, undefined, { ...node.attrs, nodeId, taskId: null });
      seen.add(nodeId);
      changed = true;
    } else seen.add(id);
  });
  return changed;
}

export const LinkedTaskItem = TaskItem.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      nodeId: { default: null, parseHTML: (element: HTMLElement) => element.getAttribute("data-node-id"), renderHTML: (attrs: Record<string, unknown>) => ({ "data-node-id": attrs.nodeId }) },
      taskId: { default: null, parseHTML: (element: HTMLElement) => element.getAttribute("data-task-id"), renderHTML: (attrs: Record<string, unknown>) => ({ "data-task-id": attrs.taskId }) },
    };
  },
  addProseMirrorPlugins() {
    return [...(this.parent?.() ?? []), new Plugin({
      appendTransaction(transactions, _oldState, state) {
        if (!transactions.some((transaction) => transaction.docChanged)) return null;
        const transaction = state.tr;
        return identifyTaskItems(state.doc, transaction) ? transaction : null;
      },
    })];
  },
});
