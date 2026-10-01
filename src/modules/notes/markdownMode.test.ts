import { expect, it } from "vitest";
import { MarkdownManager } from "@tiptap/markdown";
import StarterKit from "@tiptap/starter-kit";
import TaskList from "@tiptap/extension-task-list";
import TaskItem from "@tiptap/extension-task-item";
import { Table } from "@tiptap/extension-table";
import TableRow from "@tiptap/extension-table-row";
import TableHeader from "@tiptap/extension-table-header";
import TableCell from "@tiptap/extension-table-cell";
import type { JSONContent } from "@tiptap/core";
import { createMarkdownMode } from "./markdownMode";

function mode() {
  return createMarkdownMode(new MarkdownManager({ extensions: [StarterKit, TaskList, TaskItem, Table, TableRow, TableHeader, TableCell] }));
}

it("parses editable Markdown headings, formatting, lists, tasks, tables and code", () => {
  const converter = mode();
  const source = '# Heading\n\n**Bold** and *italic*\n\n- Item\n\n- [x] Done\n\n| A | B |\n| --- | --- |\n| C | D |\n\n```js\nconst x = 1;\n```';
  const doc = converter.parse(source);
  expect(doc.content?.map((node) => node.type)).toEqual(["heading", "paragraph", "bulletList", "taskList", "table", "codeBlock"]);
  expect(doc.content?.[1].content?.[0].marks).toEqual([{ type: "bold" }]);
  expect(doc.content?.[3].content?.[0].attrs?.checked).toBe(true);
  expect(converter.serialize(doc)).toContain("# Heading");
  expect(converter.parse("").content ?? []).toEqual([]);
});

it("keeps custom nodes, attachment IDs, task identities and styles when surrounding source changes", () => {
  const converter = mode();
  const special: JSONContent[] = [
    { type: "image", attrs: { attachmentId: "img-id", src: "attachment://img-id", width: "120px" } },
    { type: "drawingBlock", attrs: { svg: '<svg><path d="M0 0"/></svg>' } },
    { type: "attachmentBlock", attrs: { attachmentId: "file-id", fileName: "report.pdf", size: 123 } },
    { type: "taskList", content: [{ type: "taskItem", attrs: { nodeId: "node", taskId: "task", checked: true }, content: [{ type: "paragraph", content: [{ type: "text", text: "Linked task" }] }] }] },
  ];
  const inline: JSONContent[] = [
    { type: "pageLink", attrs: { pageId: "page", label: "Target" } },
    { type: "text", text: "Styled", marks: [{ type: "textStyle", attrs: { color: "#ff0000", fontSize: "22px" } }] },
  ];
  const source = converter.serialize({ type: "doc", content: [
    { type: "paragraph", content: [{ type: "text", text: "Before " }, ...inline] }, ...special,
  ] });
  const restored = converter.parse(source.replace("Before", "After"));
  expect(restored.content?.slice(1)).toEqual(special);
  expect(restored.content?.[0].content?.slice(1)).toEqual(inline);
  expect(restored.content?.[0].content?.[0].text).toBe("After ");
  expect(converter.parse(source)).toEqual(converter.parse(source));
});

it("allows deleting references and leaves ordinary code alone", () => {
  const converter = mode();
  const source = converter.serialize({ type: "doc", content: [{ type: "drawingBlock", attrs: { svg: "drawing" } }] });
  expect(source).toContain("```tenjee");
  expect(converter.parse("Replacement").content?.[0].type).toBe("paragraph");
  expect(converter.parse("```tenjee\nmy code\n```").content?.[0].type).toBe("codeBlock");
});
