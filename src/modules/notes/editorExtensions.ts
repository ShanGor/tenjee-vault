import { ui } from "../../i18n/ui";
import StarterKit from "@tiptap/starter-kit";
import { Markdown } from "@tiptap/markdown";
import TaskList from "@tiptap/extension-task-list";
import { LinkedTaskItem } from "./LinkedTaskItem";
import { Extension, generateHTML, type Extensions } from "@tiptap/core";
import { Plugin } from "@tiptap/pm/state";
import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { markdownTable } from "./MarkdownTable";
import { TableRow } from "@tiptap/extension-table-row";
import { TableCell } from "@tiptap/extension-table-cell";
import { TableHeader } from "@tiptap/extension-table-header";
import Placeholder from "@tiptap/extension-placeholder";
import Highlight from "@tiptap/extension-highlight";
import { TextStyle, Color, FontSize } from "@tiptap/extension-text-style";
import { NoteCodeBlock } from "./NoteCodeBlock";
import { Details, DetailsContent, DetailsSummary } from "@tiptap/extension-details";
import { api } from "./api";
import { AttachmentBlock, AttachmentImage, DrawingBlock, PageLink } from "./extensions";

const EditorLinkClicks = Extension.create({
  name: "editorLinkClicks",
  addProseMirrorPlugins() {
    return [new Plugin({
      props: {
        handleDOMEvents: {
          click: (view, event) => {
            if (!view.editable || event.button !== 0) return false;
            const link = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>("a[href]") : null;
            if (!link || !view.dom.contains(link)) return false;
            // Also cancel native navigation and Tauri's automatic link opener.
            event.preventDefault();
            if (event.ctrlKey || event.metaKey) {
              if (isTauri()) void openUrl(link.href).catch(console.error);
              else window.open(link.href, "_blank", "noopener,noreferrer");
            }
            return true;
          },
        },
      },
    })];
  },
});

export function buildEditorExtensions(spaceId: string) {
  const extensions: Extensions = [
    StarterKit.configure({ codeBlock: false, link: { openOnClick: false } }),
    EditorLinkClicks,
    NoteCodeBlock,
    Details.configure({
      persist: true,
      renderToggleButton: ({ element, isOpen }) => {
        element.setAttribute("aria-label", ui(isOpen ? "折叠区块" : "展开区块"));
        element.setAttribute("aria-expanded", String(isOpen));
      },
    }),
    DetailsSummary,
    DetailsContent,
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
    Placeholder.configure({ placeholder: ({ node }) => node.type.name === "detailsSummary"
      ? ui("区块标题") : ui("开始书写…（输入 [[ 插入页面链接）") }),
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
