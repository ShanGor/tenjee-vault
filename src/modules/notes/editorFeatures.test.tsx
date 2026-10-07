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
afterEach(() => { act(() => root.unmount()); editor.destroy(); host.remove(); setUILocale("en"); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

function toolbar() {
  act(() => root.render(<><EditorToolbar editor={editor} onPickAttachment={() => undefined} disabled={false} /><EditorContent editor={editor} /></>));
}
function click(label: string) {
  const button = Array.from(document.body.querySelectorAll("button")).find((button) => button.textContent === label || button.getAttribute("aria-label") === label);
  expect(button, label).toBeDefined();
  expect(button?.disabled, label).toBe(false);
  act(() => button!.click());
}

function field(label: string, value: string) {
  const element = Array.from(document.querySelectorAll("label")).find((item) => item.textContent === label)!;
  const input = document.getElementById(element.htmlFor) as HTMLInputElement;
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  return input;
}

it("keeps table actions available and enables row/column operations when the cursor enters a table", () => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  toolbar();
  click("表格操作");
  expect(document.querySelector(".editor-table-hint")?.textContent).toContain("将光标放入表格");
  expect(document.querySelector<HTMLButtonElement>('.floating-action-menu button')?.disabled).toBe(true);
  click("表格操作");
  act(() => editor.commands.insertTable({ rows: 3, cols: 3, withHeaderRow: true }));
  expect(host.querySelector(".editor-table-menu.is-active")).not.toBeNull();
  click("表格操作");
  for (const label of ["插入列", "插入行", "删除表格"]) {
    expect(Array.from(document.querySelectorAll<HTMLButtonElement>(".floating-action-menu button")).find((button) => button.textContent === label)?.disabled).toBe(false);
  }
  click("插入列");
  expect(editor.state.doc.firstChild!.child(0).childCount).toBe(4);
});

it("preserves a multi-cell selection through the table menu and merges the selected cells", () => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  toolbar();
  act(() => editor.commands.insertTable({ rows: 2, cols: 2, withHeaderRow: false }));
  const cells: number[] = [];
  editor.state.doc.descendants((node, pos) => { if (node.type.name === "tableCell") cells.push(pos); });
  act(() => editor.view.dispatch(editor.state.tr.setSelection(new CellSelection(editor.state.doc.resolve(cells[0]), editor.state.doc.resolve(cells[1])))));
  const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="表格操作"]')!;
  const mouseDown = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
  act(() => trigger.dispatchEvent(mouseDown));
  expect(mouseDown.defaultPrevented).toBe(true);
  click("表格操作"); click("合并单元格");
  expect(editor.state.doc.firstChild!.child(0).childCount).toBe(1);
  expect(editor.state.doc.firstChild!.child(0).firstChild!.attrs.colspan).toBe(2);
});

it("inserts a link at an empty cursor with display text and a normalized address", () => {
  toolbar();
  act(() => editor.commands.setTextSelection(6));
  click("链接");
  field("显示文字", "Example"); field("链接地址", "example.com");
  click("插入链接");
  expect(editor.getText()).toBe("HelloExample");
  expect(editor.getHTML()).toContain('href="https://example.com"');
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  // Text typed after saving should not inherit the newly inserted link.
  act(() => editor.commands.insertContent(" next"));
  expect(editor.getHTML()).toContain("Example</a> next");
});

it("uses selected text for a new link and inserts the address when display text is empty", () => {
  toolbar();
  act(() => editor.commands.setTextSelection({ from: 1, to: 6 }));
  click("链接");
  expect(document.querySelector<HTMLInputElement>('.editor-link-dialog input')?.value).toBe("Hello");
  field("链接地址", "person@example.com"); click("插入链接");
  expect(editor.getText()).toBe("Hello");
  expect(editor.getHTML()).toContain('href="mailto:person@example.com"');
  act(() => editor.commands.setContent("<p></p>"));
  click("链接"); field("链接地址", "https://example.com"); click("插入链接");
  expect(editor.getText()).toBe("https://example.com");
  expect(editor.getHTML()).toContain('href="https://example.com"');
});

it("edits the whole existing link from a cursor inside it and supports undo/redo", () => {
  toolbar();
  act(() => { editor.commands.setContent('<p>Before <a href="https://old.example">Old label</a> after</p>'); editor.commands.setTextSelection(10); });
  expect(host.querySelector(".editor-link-address")?.textContent).toBe("https://old.example");
  click("编辑链接");
  expect(document.querySelector<HTMLInputElement>('.editor-link-dialog input')?.value).toBe("Old label");
  field("显示文字", "New label"); field("链接地址", "https://new.example"); click("保存");
  expect(editor.getText()).toBe("Before New label after");
  expect(editor.getHTML()).toContain('href="https://new.example"');
  click("撤销");
  expect(editor.getText()).toBe("Before Old label after");
  expect(editor.getHTML()).toContain('href="https://old.example"');
  click("重做");
  expect(editor.getText()).toBe("Before New label after");
});

it("keeps mixed text formatting when updating only a link address", () => {
  toolbar();
  act(() => { editor.commands.setContent('<p><a href="https://old.example"><strong>Bold</strong> plain</a></p>'); editor.commands.setTextSelection(3); });
  click("编辑链接"); field("链接地址", "https://new.example"); click("保存");
  expect(editor.getText()).toBe("Bold plain");
  const nodes = editor.getJSON().content![0].content!;
  expect(nodes[0].marks?.find((mark) => mark.type === "bold")).toBeDefined();
  expect(nodes[1].marks?.find((mark) => mark.type === "bold")).toBeUndefined();
  for (const node of nodes) expect(node.marks?.find((mark) => mark.type === "link")?.attrs?.href).toBe("https://new.example");
});

it("unlinks a raw URL without deleting its text or immediately autolinking it again", () => {
  toolbar();
  act(() => { editor.commands.setContent('<p><a href="https://example.com">https://example.com</a></p>'); editor.commands.setTextSelection(5); });
  const text = editor.getText();
  const html = editor.getHTML();
  const unlink = host.querySelector<HTMLButtonElement>('.editor-link-context button:last-child')!;
  act(() => unlink.click());
  expect(editor.getText()).toBe(text);
  expect(editor.getHTML()).not.toContain("<a");
  expect(host.querySelector(".editor-link-context")).toBeNull();
  click("撤销"); expect(editor.getHTML()).toBe(html);
  act(() => editor.commands.setTextSelection(5));
  click("编辑链接"); click("取消链接");
  expect(editor.getText()).toBe(text);
  expect(editor.getHTML()).not.toContain("<a");
});

it("rejects unsafe and empty addresses without changing text or removing an existing link", () => {
  toolbar();
  act(() => { editor.commands.setContent('<p><a href="https://old.example">Old label</a></p>'); editor.commands.setTextSelection(3); });
  const html = editor.getHTML();
  click("编辑链接"); field("显示文字", "Changed"); field("链接地址", "javascript:alert(1)"); click("保存");
  expect(document.querySelector('[role="alert"]')?.textContent).toBe("请输入有效的链接地址");
  expect(editor.getHTML()).toBe(html);
  field("链接地址", ""); click("保存");
  expect(editor.getHTML()).toBe(html);
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
});

it("opens link editing with Ctrl/Cmd+K, traps focus and restores the selection on Escape", () => {
  toolbar();
  act(() => editor.commands.setTextSelection({ from: 2, to: 5 }));
  const html = editor.getHTML();
  const key = new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true, cancelable: true });
  act(() => editor.view.dom.dispatchEvent(key));
  expect(key.defaultPrevented).toBe(true);
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  const first = document.querySelector<HTMLButtonElement>('.editor-link-dialog header button')!;
  const last = document.querySelector<HTMLButtonElement>('.editor-link-dialog button[type="submit"]')!;
  last.focus();
  act(() => last.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true })));
  expect(document.activeElement).toBe(first);
  field("显示文字", "Discarded");
  act(() => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })));
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(editor.getHTML()).toBe(html);
  expect(editor.state.selection.from).toBe(2); expect(editor.state.selection.to).toBe(5);
});

it("disables table and link actions when editing is unavailable", () => {
  act(() => root.render(<EditorToolbar editor={editor} onPickAttachment={() => undefined} disabled />));
  expect(host.querySelector<HTMLButtonElement>('button[aria-label="表格操作"]')?.disabled).toBe(true);
  expect(host.querySelector<HTMLButtonElement>('button[aria-label="链接"]')?.disabled).toBe(true);
  expect(host.querySelector<HTMLButtonElement>('button[aria-label="可折叠区块"]')?.disabled).toBe(true);
});

it("wraps selected blocks in an editable section, persists collapse state and unwraps without losing content", async () => {
  editor.setOptions({ editorProps: { handleScrollToSelection: () => true } });
  toolbar();
  act(() => {
    editor.commands.setContent('<p>Hello</p><p><strong>World</strong></p>');
    editor.commands.setTextSelection({ from: 1, to: 13 });
  });
  click("可折叠区块");
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 5)); });
  expect(editor.getJSON().content![0].type).toBe("details");
  expect(editor.getJSON().content![0].attrs?.open).toBe(true);
  expect(host.querySelector('button[aria-label="可折叠区块"]')?.getAttribute("aria-pressed")).toBe("true");
  act(() => editor.commands.insertContent("Title"));
  expect(host.querySelector("summary")?.textContent).toBe("Title");
  click("折叠区块");
  expect(host.querySelector('[data-type="detailsContent"]')?.hasAttribute("hidden")).toBe(true);
  expect(editor.getJSON().content![0].attrs?.open).toBe(false);
  expect(host.querySelector('button[aria-label="展开区块"]')?.getAttribute("aria-expanded")).toBe("false");
  const saved = editor.getJSON();
  act(() => editor.commands.setContent(saved));
  expect(host.querySelector('[data-type="detailsContent"]')?.hasAttribute("hidden")).toBe(true);
  click("展开区块");
  expect(host.querySelector('[data-type="detailsContent"]')?.hasAttribute("hidden")).toBe(false);
  act(() => editor.commands.setTextSelection(3));
  const wrapped = editor.getJSON();
  click("可折叠区块");
  expect(editor.getHTML()).toBe("<p>Title</p><p>Hello</p><p><strong>World</strong></p><p></p>");
  click("撤销");
  expect(editor.getJSON()).toEqual(wrapped);
  click("重做");
  expect(editor.getJSON().content?.some((node) => node.type === "details")).toBe(false);
});

it("allows section toggling in reading mode without changing the saved document", () => {
  toolbar();
  act(() => {
    editor.commands.setContent('<details><summary>Title</summary><div data-type="detailsContent"><p>Body</p></div></details>');
    editor.setEditable(false);
  });
  const saved = editor.getJSON();
  click("展开区块");
  expect(host.querySelector('[data-type="detailsContent"]')?.hasAttribute("hidden")).toBe(false);
  click("折叠区块");
  expect(host.querySelector('[data-type="detailsContent"]')?.hasAttribute("hidden")).toBe(true);
  expect(editor.getJSON()).toEqual(saved);
});

it("keeps nested sections, formatting and protected references through Markdown mode", () => {
  const converter = createMarkdownMode(editor.markdown!);
  const source = formatMarkdown("**Bold**\n\n- Item", 0, 16, "details").source;
  editor.commands.setContent(converter.parse(source));
  expect(editor.getJSON().content![0].type).toBe("details");
  expect(editor.getJSON().content![0].attrs?.open).toBe(true);
  expect(editor.getHTML()).toContain("<strong>Bold</strong>");
  const nested = editor.getJSON().content![0];
  const doc = { type: "doc", content: [{ ...nested, attrs: { open: false }, content: [
    { type: "detailsSummary", content: [{ type: "text", text: "Outer" }] },
    { type: "detailsContent", content: [nested,
      { type: "attachmentBlock", attrs: { attachmentId: "file", fileName: "report.pdf", size: 123 } },
      { type: "paragraph", content: [{ type: "text", text: "Styled", marks: [{ type: "highlight" }] }] },
    ] },
  ] }] };
  const markdown = converter.serialize(doc);
  expect(markdown).toContain(":::details");
  expect(editor.schema.nodeFromJSON(converter.parse(markdown)).toJSON()).toEqual(editor.schema.nodeFromJSON(doc).toJSON());
  const inserted = formatMarkdown("", 0, 0, "details");
  expect(inserted.source.slice(inserted.selectionStart, inserted.selectionEnd)).toBe("区块标题");
});

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
