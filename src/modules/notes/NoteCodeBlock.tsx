import { useEffect, useId, useState } from "react";
import CodeBlockLowlight from "@tiptap/extension-code-block-lowlight";
import { common, createLowlight } from "lowlight";
import { NodeViewContent, NodeViewWrapper, ReactNodeViewRenderer, useEditorState, type NodeViewProps } from "@tiptap/react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { ui } from "../../i18n/ui";

const lowlight = createLowlight(common);
let mermaidReady: Promise<typeof import("mermaid")> | undefined;

function MermaidPreview({ source }: { source: string }) {
  const id = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const [result, setResult] = useState<{ source: string; svg?: string; error?: string }>();
  useEffect(() => {
    let cancelled = false;
    mermaidReady ??= import("mermaid").then((module) => {
      module.default.initialize({ startOnLoad: false, securityLevel: "strict", suppressErrorRendering: true });
      return module;
    });
    // Debounce while typing; Mermaid queues concurrent renders internally.
    const timer = setTimeout(() => {
      void mermaidReady!.then(async ({ default: mermaid }) => {
        await mermaid.parse(source);
        if (cancelled) return;
        const { svg } = await mermaid.render(`note-mermaid-${id}-${crypto.randomUUID()}`, source);
        if (!cancelled) setResult({ source, svg });
      }).catch((error: unknown) => {
        if (!cancelled) setResult({ source, error: error instanceof Error ? error.message : String(error) });
      });
    }, 300);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [source, id]);
  if (!result || result.source !== source) return <p className="text-sm text-neutral-500">{ui("正在绘制图表…")}</p>;
  if (result.error) return <p role="status" className="text-sm text-red-600">{ui("图表语法错误：{p0}", { p0: result.error })}</p>;
  return <div className="mermaid-preview" dangerouslySetInnerHTML={{ __html: result.svg ?? "" }} />;
}

function CodeBlockView({ editor, node, updateAttributes }: NodeViewProps) {
  const editable = useEditorState({ editor, selector: ({ editor }) => editor.isEditable });
  const language = String(node.attrs.language ?? "");
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);
  const languageList = useId();
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);
  async function copyCode() {
    try {
      if ("__TAURI_INTERNALS__" in window) await writeText(node.textContent);
      else await navigator.clipboard.writeText(node.textContent);
      setCopied(true); setCopyError(false);
    } catch { setCopyError(true); }
  }
  return <NodeViewWrapper className="note-code-block">
    <div className="note-code-header" contentEditable={false}>
      {editable ? <>
        <input aria-label={ui("代码语言")} list={languageList} value={language} placeholder={ui("纯文本")}
          onChange={(event) => updateAttributes({ language: event.target.value || null })} />
        <datalist id={languageList}><option value="mermaid" />{lowlight.listLanguages().map((name) => <option key={name} value={name} />)}</datalist>
      </> : <span>{language || ui("纯文本")}</span>}
      <button type="button" onClick={() => void copyCode()}>{ui(copied ? "已复制" : "复制代码")}</button>
      {copyError && <span role="status">{ui("复制失败")}</span>}
    </div>
    <pre><NodeViewContent<"code"> as="code" className={language ? `language-${language}` : undefined} /></pre>
    {language === "mermaid" && node.textContent.trim() && <div contentEditable={false}><MermaidPreview source={node.textContent} /></div>}
  </NodeViewWrapper>;
}

export const NoteCodeBlock = CodeBlockLowlight.extend({
  addNodeView() { return ReactNodeViewRenderer(CodeBlockView); },
  renderMarkdown(node) {
    const code = node.content?.map((child) => child.text ?? "").join("") ?? "";
    const fence = "`".repeat(Math.max(2, ...(code.match(/`+/g) ?? []).map((run) => run.length)) + 1);
    return `${fence}${node.attrs?.language || ""}\n${code}\n${fence}`;
  },
}).configure({ lowlight, enableTabIndentation: true, tabSize: 2 });
