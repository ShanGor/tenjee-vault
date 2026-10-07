// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Editor } from "@tiptap/core";
import { EditorContent } from "@tiptap/react";
import { CellSelection } from "@tiptap/pm/tables";
import { EditorToolbar } from "./EditorToolbar";
import { MarkdownEditor, formatMarkdown } from "./MarkdownEditor";
import { buildEditorExtensions } from "./editorExtensions";
import { createMarkdownMode } from "./markdownMode";
import { setUILocale } from "../../i18n/ui";

let root: Root;
let host: HTMLDivElement;
let editor: Editor;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  setUILocale("zh-CN");
  host = document.createElement("div"); document.body.append(host);
  root = createRoot(host);
  editor = new Editor({ extensions: buildEditorExtensions("space"), content: { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: "Hello" }] }] } });
});
afterEach(() => { act(() => root.unmount()); editor.destroy(); host.remove(); setUILocale("en"); vi.restoreAllMocks(); });

function toolbar() {
  act(() => root.render(<><EditorToolbar editor={editor} onPickAttachment={() => undefined} disabled={false} /><EditorContent editor={editor} /></>));
}
function click(label: string) {
  const button = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === label || button.getAttribute("aria-label") === label);
  expect(button, label).toBeDefined();
  expect(button?.disabled, label).toBe(false);
  act(() => button!.click());
}

it("applies quote and code from the toolbar, updates active state and supports undo/redo", () => {
  toolbar();
  act(() => editor.commands.setTextSelection({ from: 1, to: 6 }));
  click("引用");
  expect(editor.getJSON().content?.[0].type).toBe("blockquote");
  expect(host.querySelector('button[aria-label="引用"]')?.getAttribute("aria-pressed")).toBe("true");
  click("引用"); click("代码块");
  expect(editor.getJSON().content?.[0].type).toBe("codeBlock");
  expect(editor.getText()).toContain("Hello");
  click("撤销");
  expect(editor.isActive("codeBlock")).toBe(false);
  click("重做");
  expect(editor.isActive("codeBlock")).toBe(true);
});

it("deletes the selected table column and row without removing the other cells, and can undo", () => {
  toolbar();
  act(() => editor.commands.setContent({ type: "doc", content: [{ type: "table", content: [
    ["A", "B", "C"], ["D", "E", "F"], ["G", "H", "I"],
  ].map((cells, index) => ({ type: "tableRow", content: cells.map((text) => ({ type: index ? "tableCell" : "tableHeader", content: [{ type: "paragraph", content: [{ type: "text", text }] }] })) })) }] }));
  const positions: number[] = [];
  editor.state.doc.descendants((node, pos) => { if (node.type.name === "tableCell") positions.push(pos); });
  act(() => editor.commands.setTextSelection(positions[1] + 2));
  click("删除列");
  let table = editor.state.doc.firstChild!;
  expect(table.childCount).toBe(3); expect(table.child(0).childCount).toBe(2);
  expect(table.textContent).toBe("ACDFGI");
  click("删除行");
  table = editor.state.doc.firstChild!;
  expect(table.childCount).toBe(2); expect(table.textContent).toBe("ACGI");
  click("撤销");
  expect(editor.state.doc.firstChild!.childCount).toBe(3);
});

it("preserves merged cells and attachments when editing surrounding Markdown", () => {
  editor.commands.setContent({ type: "doc", content: [{ type: "table", content: [
    { type: "tableRow", content: ["A", "B"].map((text) => ({ type: "tableHeader", content: [{ type: "paragraph", content: [{ type: "text", text }] }] })) },
    { type: "tableRow", content: ["C", "D"].map((text) => ({ type: "tableCell", content: [{ type: "paragraph", content: [{ type: "text", text }] }] })) },
  ] }] });
  const cells: number[] = [];
  editor.state.doc.descendants((node, pos) => { if (node.type.name === "tableCell") cells.push(pos); });
  editor.view.dispatch(editor.state.tr.setSelection(new CellSelection(editor.state.doc.resolve(cells[0]), editor.state.doc.resolve(cells[1]))));
  expect(editor.commands.mergeCells()).toBe(true);
  const merged = editor.getJSON().content![0];
  const mode = createMarkdownMode(editor.markdown!);
  const source = mode.serialize(editor.getJSON());
  expect(source).toContain("<table");
  expect(source).toContain('colspan="2"');
  expect(source).not.toContain("```tenjee");
  const fragment = mode.serializeFragment({ type: "doc", content: [{ type: "image", attrs: { src: "attachment://img", attachmentId: "img" } }] });
  const roundtrip = mode.parse(`${source}\n\nAdded text\n\n${fragment}`);
  expect(roundtrip.content![0]).toEqual(merged);
  expect(roundtrip.content!.find((node) => node.type === "image")?.attrs?.attachmentId).toBe("img");
  const edited = mode.parse(source.replace("<p>C</p>", "<p>Edited</p>"));
  expect(edited.content![0].content![1].content![0].attrs?.colspan).toBe(2);
  expect(edited.content![0].content![1].content![0].content![0].content![0].text).toBe("Edited");
});

it("roundtrips HTML row spans, header columns and block content mixed with ordinary Markdown", () => {
  const mode = createMarkdownMode(editor.markdown!);
  const html = '<table><tbody><tr><th rowspan="2"><p><strong>A &amp; B</strong></p></th><td><p>First</p><p>Second</p><ul><li><p>Item</p></li></ul></td></tr><tr><td><p>Last</p></td></tr></tbody></table>';
  editor.commands.setContent(mode.parse(`# Before\n\n${html}\n\nAfter`));
  const doc = editor.getJSON();
  const source = mode.serialize(doc);
  expect(source).toContain('rowspan="2"');
  expect(source).toContain("<strong>A &amp; B</strong>");
  expect(source).toContain("<ul>");
  expect(source).toContain("# Before");
  expect(editor.schema.nodeFromJSON(mode.parse(source)).toJSON()).toEqual(doc);
});

it("roundtrips GFM, six heading levels, links, image URLs, nested quotes and literal fenced code", () => {
  const mode = createMarkdownMode(editor.markdown!);
  const source = '###### Heading\n\n> Quote\n>\n> > Nested\n\n~~deleted~~ and `inline` [site](https://example.com)\n\n![image](https://example.com/image.png)\n\n| Left | Right |\n| :--- | ---: |\n| A | B |\n\n````markdown\n```js\nconst n = 1;\n```\n````';
  editor.commands.setContent(mode.parse(source));
  const doc = editor.getJSON();
  expect(doc.content![0].attrs?.level).toBe(6);
  expect(doc.content![1].type).toBe("blockquote");
  expect(doc.content![2].content![0].marks?.[0].type).toBe("strike");
  expect(mode.serialize(doc)).toContain("![image](https://example.com/image.png)");
  const restored = mode.parse(mode.serialize(doc));
  expect(restored.content!.find((node) => node.type === "codeBlock")).toEqual(doc.content!.find((node) => node.type === "codeBlock"));
  expect(editor.schema.nodeFromJSON(restored).toJSON().content!.find((node: { type: string }) => node.type === "table"))
    .toEqual(doc.content!.find((node) => node.type === "table"));
});

it("renders syntax highlighting and normal Markdown images", () => {
  toolbar();
  act(() => editor.commands.setContent(createMarkdownMode(editor.markdown!).parse('```js\nconst answer = 42;\n```\n\n![sample](https://example.com/sample.png)')));
  expect(host.querySelector(".hljs-keyword")?.textContent).toBe("const");
  expect(host.querySelector("img")?.getAttribute("src")).toBe("https://example.com/sample.png");
  expect(host.querySelector("img")?.alt).toBe("sample");
});

it("formats complete selected lines and uses safe delimiters for embedded backticks", () => {
  expect(formatMarkdown("first\nsecond\nthird", 2, 12, "quote").source).toBe("> first\n> second\nthird");
  expect(formatMarkdown("a\nb", 0, 2, "bullet").source).toBe("- a\nb");
  expect(formatMarkdown("x `y`", 0, 5, "code").source).toBe("`` x `y` ``");
  const result = formatMarkdown("```js\nx\n```", 0, 11, "codeBlock");
  expect(result.source).toBe("````text\n```js\nx\n```\n````\n");
});

it("inserts uploaded image references at the Markdown selection after asynchronous upload", async () => {
  const change = vi.fn();
  const files = vi.fn(async (_files: File[], insert: (source: string) => void) => { await Promise.resolve(); insert("image-reference"); });
  act(() => root.render(<MarkdownEditor value="before selected after" onChange={change} onFiles={files} onPasteText={() => undefined} disabled={false} />));
  const textarea = host.querySelector("textarea")!;
  textarea.setSelectionRange(7, 15);
  const paste = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(paste, "clipboardData", { value: { files: [new File(["image"], "sample.png", { type: "image/png" })] } });
  await act(async () => { textarea.dispatchEvent(paste); await Promise.resolve(); });
  expect(paste.defaultPrevented).toBe(true);
  expect(change).toHaveBeenCalledWith("before \n\nimage-reference\n\n after");
});
