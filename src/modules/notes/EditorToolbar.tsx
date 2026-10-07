import { useEffect, useState } from "react";
import type { Editor } from "@tiptap/core";
import { useEditorState } from "@tiptap/react";
import { ActionMenu } from "../../shared/ActionMenu";
import { ui } from "../../i18n/ui";
import { EditorLinkDialog, removeEditorLink, selectedLink } from "./EditorLinkDialog";

export function EditorToolbar({ editor, onPickAttachment, disabled }: {
  editor: Editor; onPickAttachment: () => void; disabled: boolean;
}) {
  // Selection changes and undo/redo must update active and available actions.
  useEditorState({ editor, selector: ({ transactionNumber }) => transactionNumber });
  const [editingLink, setEditingLink] = useState(false);
  const link = selectedLink(editor);
  useEffect(() => { if (disabled) setEditingLink(false); }, [disabled]);
  useEffect(() => {
    const keyboard = (event: KeyboardEvent) => {
      if (!disabled && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault(); setEditingLink(true);
      }
    };
    const element = editor.view.dom;
    element.addEventListener("keydown", keyboard);
    return () => element.removeEventListener("keydown", keyboard);
  }, [editor, disabled]);
  const btn = "rounded border px-2 py-1 text-sm disabled:opacity-40";
  const chain = () => editor.chain().focus();
  const active = (name: string, attrs?: object) => editor.isActive(name, attrs);
  const toggle = (label: string, name: string, action: () => void, content = label) => (
    <button type="button" className={btn} disabled={disabled} aria-label={label} title={label}
      aria-pressed={active(name)} onMouseDown={(event) => event.preventDefault()} onClick={action}>{content}</button>
  );
  const tableActions = [
    ["在前面插入列", "addColumnBefore"], ["插入列", "addColumnAfter"], ["删除列", "deleteColumn"],
    ["在上方插入行", "addRowBefore"], ["插入行", "addRowAfter"], ["删除行", "deleteRow"],
    ["合并单元格", "mergeCells"], ["拆分单元格", "splitCell"],
    ["切换表头行", "toggleHeaderRow"], ["切换表头列", "toggleHeaderColumn"], ["删除表格", "deleteTable"],
  ] as const;

  return <><div className="page-formatting-toolbar flex flex-wrap items-center gap-1 border-b" role="group" aria-label={ui("格式工具栏")}>
    <button type="button" className={btn} disabled={disabled || !editor.can().undo()} onClick={() => chain().undo().run()}>{ui("撤销")}</button>
    <button type="button" className={btn} disabled={disabled || !editor.can().redo()} onClick={() => chain().redo().run()}>{ui("重做")}</button>
    <select className={btn} aria-label={ui("段落格式")} disabled={disabled}
      value={[1, 2, 3, 4, 5, 6].find((level) => active("heading", { level })) ?? 0}
      onChange={(event) => {
        const level = Number(event.target.value) as 1 | 2 | 3 | 4 | 5 | 6;
        if (level) chain().setHeading({ level }).run(); else chain().setParagraph().run();
      }}>
      <option value="0">{ui("正文")}</option>
      {(["标题 1", "标题 2", "标题 3", "标题 4", "标题 5", "标题 6"] as const).map((label, index) => <option key={label} value={index + 1}>{ui(label)}</option>)}
    </select>
    {toggle(ui("加粗"), "bold", () => chain().toggleBold().run(), "B")}
    {toggle(ui("斜体"), "italic", () => chain().toggleItalic().run(), "I")}
    {toggle(ui("下划线"), "underline", () => chain().toggleUnderline().run(), "U")}
    {toggle(ui("删除线"), "strike", () => chain().toggleStrike().run(), "S")}
    {toggle(ui("行内代码"), "code", () => chain().toggleCode().run())}
    {toggle(ui("高亮"), "highlight", () => chain().toggleHighlight().run())}
    <label className={`${btn} flex items-center gap-1`} title={ui("字体颜色")}>
      <span>A</span><input type="color" aria-label={ui("字体颜色")} className="h-4 w-6" disabled={disabled}
        value={editor.getAttributes("textStyle").color || "#273b32"} onChange={(event) => chain().setColor(event.target.value).run()} />
    </label>
    <select className={btn} aria-label={ui("字号")} disabled={disabled} value={editor.getAttributes("textStyle").fontSize || ""}
      onChange={(event) => event.target.value ? chain().setFontSize(event.target.value).run() : chain().unsetFontSize().run()}>
      <option value="">{ui("字号")}</option>
      {["12px", "14px", "16px", "18px", "22px", "28px"].map((size) => <option key={size}>{size}</option>)}
    </select>
    {toggle(ui("引用"), "blockquote", () => chain().toggleBlockquote().run())}
    <button type="button" className={btn} aria-label={ui("可折叠区块")} aria-pressed={active("details")}
      title={ui(active("details") ? "取消折叠区块" : "可折叠区块")}
      disabled={disabled || !(active("details") ? editor.can().unsetDetails() : editor.can().setDetails())}
      onMouseDown={(event) => event.preventDefault()} onClick={() => {
        if (active("details")) chain().unsetDetails().run();
        else chain().setDetails().updateAttributes("details", { open: true }).run();
      }}>{ui("可折叠区块")}</button>
    {toggle(ui("代码块"), "codeBlock", () => chain().toggleCodeBlock().run())}
    <button type="button" className={btn} disabled={disabled} onClick={() => chain().insertContent({ type: "codeBlock", attrs: { language: "mermaid" }, content: [{ type: "text", text: "graph TD\n  A --> B" }] }).run()}>{ui("Mermaid 图表")}</button>
    <button type="button" className={btn} disabled={disabled} onClick={() => chain().setHorizontalRule().run()}>{ui("分隔线")}</button>
    <button type="button" className={btn} disabled={disabled} aria-label={ui(link ? "编辑链接" : "链接")} aria-pressed={!!link} aria-haspopup="dialog"
      title={`${ui(link ? "编辑链接" : "插入链接")} (Ctrl/Cmd+K)`} onMouseDown={(event) => event.preventDefault()} onClick={() => setEditingLink(true)}>{ui("链接")}</button>
    {toggle(ui("• 列表"), "bulletList", () => chain().toggleBulletList().run())}
    {toggle(ui("1. 列表"), "orderedList", () => chain().toggleOrderedList().run())}
    {toggle(ui("☑ 待办"), "taskList", () => chain().toggleTaskList().run())}
    <button type="button" className={btn} disabled={disabled} onClick={() => chain().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run()}>{ui("表格")}</button>
    <ActionMenu label={ui("表格操作")} trigger={<>{ui("表格操作")} <span aria-hidden="true">▾</span></>} disabled={disabled} preserveSelection className={`editor-table-menu ${active("table") ? "is-active" : ""}`}>
      {!active("table") && <p className="editor-table-hint">{ui("将光标放入表格以操作行、列和单元格")}</p>}
      {tableActions.map(([label, command], index) => <button type="button" key={command} disabled={disabled || !editor.can()[command]()}
        className={`${[3, 6, 8, 10].includes(index) ? "editor-table-divider" : ""} ${command === "deleteTable" ? "editor-table-delete" : ""}`}
        onClick={() => chain()[command]().run()}>{ui(label)}</button>)}
    </ActionMenu>
    {active("table") && <>
      <button type="button" className={btn} disabled={disabled || !editor.can().deleteColumn()} onClick={() => chain().deleteColumn().run()}>{ui("删除列")}</button>
      <button type="button" className={btn} disabled={disabled || !editor.can().deleteRow()} onClick={() => chain().deleteRow().run()}>{ui("删除行")}</button>
    </>}
    <button type="button" className={btn} disabled={disabled} onClick={() => chain().unsetAllMarks().clearNodes().run()}>{ui("清除格式")}</button>
    <button type="button" className={btn} disabled={disabled} onClick={() => chain().insertDrawingBlock().run()}>{ui("✏️ 绘图")}</button>
    <button type="button" className={btn} disabled={disabled} onClick={onPickAttachment}>{ui("📎 附件")}</button>
  </div>
    {link && !disabled && <div className="editor-link-context" role="group" aria-label={ui("链接操作")}>
      <span className="editor-link-address" title={link.href}>{link.href}</span>
      <button type="button" aria-haspopup="dialog" onMouseDown={(event) => event.preventDefault()} onClick={() => setEditingLink(true)}>{ui("编辑链接")}</button>
      <button type="button" title={ui("保留文字，取消链接")} onMouseDown={(event) => event.preventDefault()} onClick={() => {
        removeEditorLink(editor, link);
      }}>{ui("取消链接")}</button>
    </div>}
    {editingLink && !disabled && <EditorLinkDialog editor={editor} onClose={() => setEditingLink(false)} />}
  </>;
}
