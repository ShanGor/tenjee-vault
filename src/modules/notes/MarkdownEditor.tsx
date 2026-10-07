import { useRef } from "react";
import { ui } from "../../i18n/ui";

type Format = "bold" | "italic" | "strike" | "code" | "quote" | "codeBlock" | "heading" | "bullet" | "ordered" | "task" | "link" | "table" | "rule" | "mermaid";

export function formatMarkdown(source: string, start: number, end: number, format: Format) {
  const selected = source.slice(start, end);
  let text: string;
  let selectionStart: number;
  let selectionEnd: number;
  const marks = { bold: "**", italic: "*", strike: "~~", code: "`" };
  if (format in marks) {
    let mark = marks[format as keyof typeof marks];
    if (format === "code") {
      const runs = selected.match(/`+/g) ?? [];
      mark = "`".repeat(Math.max(0, ...runs.map((run) => run.length)) + 1);
      if (selected.startsWith("`") || selected.endsWith("`")) mark += " ";
    }
    const close = mark.trim();
    text = mark + (selected || "text") + (mark.endsWith(" ") ? " " : "") + close;
    selectionStart = start + mark.length;
    selectionEnd = selectionStart + (selected || "text").length;
  } else if (["quote", "heading", "bullet", "ordered", "task"].includes(format)) {
    const lastSelectedPosition = end > start && source[end - 1] === "\n" ? end - 1 : end;
    start = start === 0 ? 0 : source.lastIndexOf("\n", start - 1) + 1;
    const lineEnd = source.indexOf("\n", lastSelectedPosition);
    end = lineEnd < 0 ? source.length : lineEnd;
    const prefixes = { quote: "> ", heading: "## ", bullet: "- ", ordered: "1. ", task: "- [ ] " };
    const prefix = prefixes[format as keyof typeof prefixes];
    text = source.slice(start, end).split("\n").map((line) => prefix + line).join("\n");
    selectionStart = start;
    selectionEnd = start + text.length;
  } else {
    const prefix = start > 0 ? (source[start - 1] === "\n" ? "\n" : "\n\n") : "";
    const suffix = end < source.length ? (source[end] === "\n" ? "\n" : "\n\n") : "\n";
    if (format === "link") {
      text = `[${selected || "text"}](https://)`;
      selectionStart = start + text.indexOf("https://");
      selectionEnd = selectionStart + 8;
    } else {
      const fence = "`".repeat(Math.max(2, ...(selected.match(/`+/g) ?? []).map((run) => run.length)) + 1);
      const blocks = {
        codeBlock: `${fence}text\n${selected || "code"}\n${fence}`,
        mermaid: `${fence}mermaid\n${selected || "graph TD\n  A --> B"}\n${fence}`,
        table: "| A | B |\n| --- | --- |\n|   |   |", rule: "---",
      };
      text = prefix + blocks[format as keyof typeof blocks] + suffix;
      selectionStart = start + prefix.length;
      selectionEnd = selectionStart + text.length - prefix.length - suffix.length;
    }
  }
  return { source: source.slice(0, start) + text + source.slice(end), selectionStart, selectionEnd };
}

export function MarkdownEditor({ value, onChange, onFiles, onPasteText, disabled }: {
  value: string; onChange: (value: string) => void;
  onFiles: (files: File[], insert: (source: string) => void) => Promise<void>;
  onPasteText: () => void; disabled: boolean;
}) {
  const textarea = useRef<HTMLTextAreaElement>(null);
  const source = useRef(value);
  source.current = value;
  const unavailable = useRef(disabled);
  unavailable.current = disabled;
  const btn = "rounded border px-2 py-1 text-sm disabled:opacity-40";
  function apply(format: Format) {
    if (!textarea.current) return;
    const result = formatMarkdown(value, textarea.current.selectionStart, textarea.current.selectionEnd, format);
    source.current = result.source;
    onChange(result.source);
    requestAnimationFrame(() => { textarea.current?.focus(); textarea.current?.setSelectionRange(result.selectionStart, result.selectionEnd); });
  }
  function upload(files: File[]) {
    const start = textarea.current?.selectionStart ?? value.length;
    const end = textarea.current?.selectionEnd ?? start;
    const original = source.current;
    return onFiles(files, (fragment) => {
      if (!textarea.current || unavailable.current) return;
      // If typing continued during upload, insert at the current cursor instead.
      const from = source.current === original ? start : textarea.current.selectionStart;
      const to = source.current === original ? end : textarea.current.selectionEnd;
      const text = `\n\n${fragment}\n\n`;
      const next = source.current.slice(0, from) + text + source.current.slice(to);
      source.current = next;
      onChange(next);
      requestAnimationFrame(() => { textarea.current?.focus(); textarea.current?.setSelectionRange(from + text.length, from + text.length); });
    });
  }
  return <>
    <div className="page-formatting-toolbar flex flex-wrap items-center gap-1 border-b" role="group" aria-label={ui("Markdown 工具栏")}>
      {([
        ["heading", "标题"], ["bold", "加粗"], ["italic", "斜体"], ["strike", "删除线"], ["code", "行内代码"],
        ["quote", "引用"], ["codeBlock", "代码块"], ["link", "链接"], ["bullet", "• 列表"], ["ordered", "1. 列表"],
        ["task", "☑ 待办"], ["table", "表格"], ["rule", "分隔线"], ["mermaid", "Mermaid 图表"],
      ] as const).map(([format, label]) => <button type="button" key={format} className={btn} disabled={disabled}
        onMouseDown={(event) => event.preventDefault()} onClick={() => apply(format)}>{ui(label)}</button>)}
      <label className={`${btn} cursor-pointer`}>
        {ui("上传图片")}
        <input type="file" accept="image/*" multiple disabled={disabled} className="sr-only" onChange={(event) => {
          const files = Array.from(event.target.files ?? []); event.target.value = "";
          if (files.length) void upload(files);
        }} />
      </label>
    </div>
    <div className="page-document-content page-markdown-content">
      <p className="mb-3 text-xs text-neutral-500">{ui("附件、绘图和特殊格式以引用保留；保留引用即可保留原内容。")}</p>
      <textarea ref={textarea} className="markdown-source" aria-label={ui("Markdown 源码")} value={value} disabled={disabled}
        onChange={(event) => { source.current = event.target.value; onChange(event.target.value); }}
        onPaste={(event) => {
          const files = Array.from(event.clipboardData.files);
          if (files.length) { event.preventDefault(); void upload(files); } else onPasteText();
        }}
        onDragOver={(event) => { if (event.dataTransfer.types.includes("Files")) event.preventDefault(); }}
        onDrop={(event) => { const files = Array.from(event.dataTransfer.files); if (files.length) { event.preventDefault(); void upload(files); } }}
        spellCheck={false} />
    </div>
  </>;
}
