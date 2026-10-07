import type { JSONContent } from "@tiptap/core";
import type { MarkdownManager } from "@tiptap/markdown";

// Markdown cannot represent attachment metadata, drawings, task identities, or
// every text style. Keep these as references while source mode is open.
export function createMarkdownMode(manager: Pick<MarkdownManager, "parse" | "serialize">) {
  const preserved = new Map<string, JSONContent>();
  const prefix = `tenjee-${crypto.randomUUID()}`;
  const standardMarks = new Set(["bold", "italic", "strike", "code", "link"]);
  function protect(node: JSONContent): JSONContent {
    const custom = ["pageLink", "drawingBlock", "attachmentBlock"].includes(node.type ?? "")
      || (node.type === "image" && !!(node.attrs?.attachmentId || node.attrs?.width));
    const styled = node.marks?.some((mark) => !standardMarks.has(mark.type));
    const linkedTasks = node.type === "taskList" && JSON.stringify(node).includes('"nodeId"');
    if (custom || styled || linkedTasks) {
      const key = `[${prefix}:${preserved.size}]`;
      preserved.set(key, structuredClone(node));
      if (node.type === "pageLink" || node.type === "text") {
        return { type: "text", text: key, marks: [{ type: "code" }] };
      }
      return { type: "codeBlock", attrs: { language: "tenjee" }, content: [{ type: "text", text: key }] };
    }
    return { ...node, ...(node.content ? { content: node.content.map(protect) } : {}) };
  }
  function restore(node: JSONContent): JSONContent {
    const key = node.type === "codeBlock" && node.attrs?.language === "tenjee"
      ? node.content?.map((child) => child.text ?? "").join("")
      : node.type === "text" && node.marks?.some((mark) => mark.type === "code") ? node.text : undefined;
    const original = key ? preserved.get(key) : undefined;
    if (original) return structuredClone(original);
    return { ...node, ...(node.content ? { content: node.content.flatMap(restoreInline) } : {}) };
  }
  function restoreInline(node: JSONContent): JSONContent[] {
    if (node.type !== "text" || !node.marks?.some((mark) => mark.type === "code")) return [restore(node)];
    // Adjacent code spans can be merged by the Markdown serializer/parser.
    const parts = (node.text ?? "").split(new RegExp(`(\\[${prefix}:\\d+\\])`, "g"));
    return parts.filter(Boolean).map((text) => {
      const original = preserved.get(text);
      return original ? structuredClone(original) : { ...node, text };
    });
  }
  return {
    serialize(doc: JSONContent) { preserved.clear(); return manager.serialize(protect(doc)); },
    // Add uploaded attachments without invalidating references already in source.
    serializeFragment(doc: JSONContent) { return manager.serialize(protect(doc)); },
    parse(source: string) { return restore(manager.parse(source)); },
  };
}
