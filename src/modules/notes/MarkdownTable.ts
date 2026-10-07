import { getExtensionField, type JSONContent, type MarkdownRendererHelpers } from "@tiptap/core";
import { Table } from "@tiptap/extension-table";

const renderGfm = getExtensionField<(node: JSONContent, helpers: MarkdownRendererHelpers) => string>(Table, "renderMarkdown");

/** GFM covers rectangular tables with one header row and inline cell content. */
export function tableNeedsHtml(node: JSONContent): boolean {
  const rows = node.content ?? [];
  return rows.some((row, rowIndex) => row.content?.some((cell, columnIndex) =>
    cell.type !== (rowIndex === 0 ? "tableHeader" : "tableCell")
    || (cell.attrs?.colspan ?? 1) !== 1 || (cell.attrs?.rowspan ?? 1) !== 1
    || !!cell.attrs?.colwidth
    || cell.content?.length !== 1 || cell.content[0].type !== "paragraph"
    || (cell.attrs?.align ?? null) !== (rows[0]?.content?.[columnIndex]?.attrs?.align ?? null),
  ) ?? false);
}

export function markdownTable(renderHtml: (node: JSONContent) => string) {
  return Table.extend({
    renderMarkdown(node, helpers) {
      if (tableNeedsHtml(node)) return renderHtml(node);
      return renderGfm(node, helpers);
    },
  }).configure({ resizable: false, renderWrapper: true });
}
