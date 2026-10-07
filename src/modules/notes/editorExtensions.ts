import { ui } from "../../i18n/ui";
import StarterKit from "@tiptap/starter-kit";
import { Markdown } from "@tiptap/markdown";
import TaskList from "@tiptap/extension-task-list";
import { LinkedTaskItem } from "./LinkedTaskItem";
import { generateHTML, type Extensions } from "@tiptap/core";
import { markdownTable } from "./MarkdownTable";
import { TableRow } from "@tiptap/extension-table-row";
import { TableCell } from "@tiptap/extension-table-cell";
import { TableHeader } from "@tiptap/extension-table-header";
import Placeholder from "@tiptap/extension-placeholder";
import Highlight from "@tiptap/extension-highlight";
import { TextStyle, Color, FontSize } from "@tiptap/extension-text-style";
import { NoteCodeBlock } from "./NoteCodeBlock";
import { api } from "./api";
import { AttachmentBlock, AttachmentImage, DrawingBlock, PageLink } from "./extensions";

export function buildEditorExtensions(spaceId: string) {
  const extensions: Extensions = [
    StarterKit.configure({ codeBlock: false, link: { openOnClick: false } }),
    NoteCodeBlock,
    Markdown,
    TaskList,
    LinkedTaskItem.configure({ nested: true }),
    markdownTable((node) => {
      const html = generateHTML({ type: "doc", content: [node] }, extensions);
      const table = new DOMParser().parseFromString(html, "text/html").querySelector("table")!;
      // Keep one row per line, without blank lines that terminate Markdown HTML blocks.
      return table.outerHTML.replace(/<tr\b/g, "\n  <tr").replace(/<\/tbody>/g, "\n</tbody>");
    }),
    TableRow,
    TableHeader,
    TableCell,
    Placeholder.configure({ placeholder: () => ui("开始书写…（输入 [[ 插入页面链接）") }),
    Highlight,
    TextStyle,
    Color,
    FontSize,
    PageLink,
    DrawingBlock,
    AttachmentBlock({
      openAttachment: (id) => api.openAttachment(spaceId, id),
      deleteAttachment: (id) => api.deleteAttachment(spaceId, id),
    }),
    // 图片渲染时经后端取附件字节（spec 5.3：文档内只存引用节点）
    AttachmentImage((id) => api.openAttachment(spaceId, id)),
  ];
  return extensions;
}
