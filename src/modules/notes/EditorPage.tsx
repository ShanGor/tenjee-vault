import { ui, uiError } from "../../i18n/ui";
// 编辑器页面：TipTap 集成、1s 防抖自动保存、格式工具栏、[[ 双链、
// 图片/附件/绘图块、版本历史面板（tasks 5.1–5.6 / 7.1）。

import { useEffect, useMemo, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import TaskList from "@tiptap/extension-task-list";
import { LinkedTaskItem } from "./LinkedTaskItem";
import { Table } from "@tiptap/extension-table";
import { TableRow } from "@tiptap/extension-table-row";
import { TableCell } from "@tiptap/extension-table-cell";
import { TableHeader } from "@tiptap/extension-table-header";
import Placeholder from "@tiptap/extension-placeholder";
import Highlight from "@tiptap/extension-highlight";
import { TextStyle, Color, FontSize } from "@tiptap/extension-text-style";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open, save } from "@tauri-apps/plugin-dialog";

import { api, BatchResult, ImportedPage, LockedError, NoteExportFormat, Page, PageTitle, PageVersion, bytesToBase64 } from "./api";
import { useNotesStore } from "./store";
import { AttachmentBlock, AttachmentImage, DrawingBlock, PageLink } from "./extensions";
import { UnlockDialog } from "./dialogs";

import { TagSelector } from "../../shared/Tags";
import { SaveTemplateButton } from "./Templates";
import { NoteTasks } from "./NoteTasks";

function buildEditorExtensions(spaceId: string) {
  return [
    StarterKit.configure({ heading: { levels: [1, 2, 3] } }),
    TaskList,
    LinkedTaskItem.configure({ nested: true }),
    Table.configure({ resizable: false }),
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
}

export default function EditorPage() {
  const { spaceId = "", sectionId = "", pageId = "" } = useParams();
  const { settings, isSectionLocked, refreshTree } = useNotesStore();
  const [page, setPage] = useState<Page | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [locked, setLocked] = useState(false);
  const [title, setTitle] = useState("");
  const [showVersions, setShowVersions] = useState(false);
  const [pageTitles, setPageTitles] = useState<PageTitle[]>([]);
  const [linkSuggest, setLinkSuggest] = useState<{ query: string; from: number } | null>(null);
  const [portableStatus, setPortableStatus] = useState<string | null>(null);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const clipboardTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const skipSave = useRef(true);

  const section = useMemo(() => {
    const tree = useNotesStore.getState().tree;
    const find = (sections: { id: string }[]) => sections.find((s) => s.id === sectionId);
    for (const nb of tree?.notebooks ?? []) {
      const hit = find(nb.sections);
      if (hit) return hit;
      for (const g of nb.groups) {
        const stack = [g];
        while (stack.length) {
          const cur = stack.pop()!;
          const h = find(cur.sections);
          if (h) return h;
          stack.push(...cur.children);
        }
      }
    }
    return null;
  }, [sectionId, useNotesStore((s) => s.tree)]);

  const sectionLocked = isSectionLocked(
    section ? { id: section.id, is_encrypted: (section as { is_encrypted?: boolean }).is_encrypted ?? false } : null,
  );
  const encryptedSection = (section as { is_encrypted?: boolean } | null)?.is_encrypted ?? false;

  // 加载页面
  useEffect(() => {
    let cancelled = false;
    setPage(null);
    setLoadError(null);
    setLocked(false);
    skipSave.current = true;
    api
      .getPage(spaceId, pageId)
      .then(async (p) => {
        if (cancelled) return;
        setPage(p);
        setTitle(p.title);
        await api.recordPageOpen(spaceId, pageId).catch(() => {});
      })
      .catch((e) => {
        if (cancelled) return;
        if (e instanceof LockedError) setLocked(true);
        else setLoadError(uiError(e));
      });
    return () => {
      cancelled = true;
    };
  }, [spaceId, pageId]);

  // [[ 双链的页面候选
  useEffect(() => {
    if (!spaceId) return;
    api.listPageTitles(spaceId).then(setPageTitles).catch(() => {});
  }, [spaceId, sectionLocked]);

  const editor = useEditor({
    editable: !sectionLocked,
    extensions: buildEditorExtensions(spaceId),
    content: "",
    editorProps: {
      handlePaste: (_view, event) => {
        const files = event.clipboardData?.files;
        if (files && files.length > 0) {
          event.preventDefault();
          handlePasteFiles(files);
          return true;
        }
        scheduleClipboardClear();
        return false;
      },
      handleDrop: (_view, event) => {
        const files = (event as DragEvent).dataTransfer?.files;
        if (files && files.length > 0) {
          event.preventDefault();
          handlePasteFiles(files);
          return true;
        }
        return false;
      },
      attributes: { class: "prose-editor" },
    },
    onUpdate: ({ editor }) => {
      if (skipSave.current) return;
      if (saveTimer.current) clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => {
        api
          .savePage(spaceId, pageId, title, JSON.stringify(editor.getJSON()))
          .catch(() => {});
      }, 1000);
      detectLinkTrigger(editor);
    },
    onSelectionUpdate: ({ editor }) => {
      detectLinkTrigger(editor);
    },
  });

  // 图片/文件粘贴与拖入
  async function handlePasteFiles(files: Iterable<File>) {
    if (!editor) return;
    for (const file of files) {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const attachment = await api.saveAttachment(
        spaceId,
        sectionId,
        pageId,
        file.name || "pasted",
        file.type || null,
        bytesToBase64(bytes),
      );
      if (file.type.startsWith("image/")) {
        editor.chain().focus().setImage({ src: `attachment://${attachment.id}`, attachmentId: attachment.id } as never).run();
      } else {
        editor
          .chain()
          .focus()
          .insertAttachmentBlock({ attachmentId: attachment.id, fileName: attachment.file_name, size: attachment.size })
          .run();
      }
    }
  }

  // 剪贴板自动清空（仅加密分区页面且设置开启）
  function scheduleClipboardClear() {
    const seconds = settings?.clipboard_auto_clear_seconds ?? 0;
    if (!encryptedSection || seconds <= 0) return;
    if (clipboardTimer.current) clearTimeout(clipboardTimer.current);
    clipboardTimer.current = setTimeout(() => {
      writeText("").catch(() => {});
    }, seconds * 1000);
  }

  // 内容载入编辑器（页面加载/回滚后）
  useEffect(() => {
    if (!editor || !page) return;
    skipSave.current = true;
    let doc = undefined;
    try {
      doc = page.content ? JSON.parse(page.content) : undefined;
    } catch {
      doc = page.content ? { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: page.content }] }] } : undefined;
    }
    editor.commands.setContent(doc ?? { type: "doc", content: [{ type: "paragraph" }] });
    skipSave.current = false;
  }, [editor, page]);

  // [[ 触发检测：光标前存在未闭合的 [[
  function detectLinkTrigger(ed: { state: any; view: any }) {
    if (!ed) return;
    const { state } = ed;
    const { $from } = state.selection;
    const textBefore = $from.parent.textBetween(0, $from.parentOffset, undefined, "\ufffc");
    const open = textBefore.lastIndexOf("[[");
    const close = textBefore.lastIndexOf("]]");
    if (open >= 0 && open > close) {
      const query = textBefore.slice(open + 2);
      if (query.length <= 30 && !query.includes("[")) {
        setLinkSuggest({ query, from: $from.pos - (query.length + 2) });
        return;
      }
    }
    setLinkSuggest(null);
  }

  function applyPageLink(target: PageTitle) {
    if (!editor || !linkSuggest) return;
    editor
      .chain()
      .focus()
      .deleteRange({ from: linkSuggest.from, to: editor.state.selection.from })
      .insertPageLink({ pageId: target.id, label: target.title })
      .run();
    setLinkSuggest(null);
  }

  async function commitTitle() {
    if (!page || title === page.title || !title.trim()) return;
    await api.renamePage(spaceId, pageId, title.trim());
    setPage({ ...page, title: title.trim() });
    refreshTree();
  }

  async function rollback(versionId: string) {
    await api.rollbackVersion(spaceId, pageId, versionId);
    const p = await api.getPage(spaceId, pageId);
    setPage(p);
  }

  async function pickAttachmentFile() {
    if (!editor) return;
    const input = document.createElement("input");
    input.type = "file";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      await handlePasteFiles([file]);
    };
    input.click();
  }

  async function importFiles() {
    const selected = await open({ multiple: true, filters: [{ name: ui("笔记文件"), extensions: ["md", "markdown", "html", "htm", "txt"] }] });
    const paths = typeof selected === "string" ? [selected] : selected ?? [];
    if (paths.length === 0) { setPortableStatus(ui("已取消导入。")); return; }
    setPortableStatus(ui("正在导入…"));
    try {
      const result = await api.importNoteFiles(spaceId, sectionId, paths);
      setPortableStatus(describeImportResult(result));
      await refreshTree();
    } catch (error) { setPortableStatus(ui("导入失败：{p0}", { p0: String(uiError(error)) })); }
  }

  async function exportCurrentPage(format: NoteExportFormat) {
    const extension = format === "markdown" ? "md" : format;
    const target = await save({ defaultPath: `${title || "page"}.${extension}`, filters: [{ name: format.toUpperCase(), extensions: [extension] }] });
    if (!target) { setPortableStatus(ui("已取消导出。")); return; }
    const destination = splitExportPath(target);
    if (!destination) { setPortableStatus(ui("导出失败：目标路径无效。")); return; }
    setPortableStatus(ui("正在导出…"));
    try {
      let confirmationToken: string | undefined;
      if (encryptedSection) {
        if (!window.confirm(ui("此导出会把加密页面写成明文文件。是否继续？"))) { setPortableStatus(ui("已取消导出。")); return; }
        confirmationToken = await api.requestPageExportConfirmation(spaceId, pageId, format, destination.directory, destination.filename);
      }
      await api.exportPage(spaceId, pageId, format, destination.directory, destination.filename, false, confirmationToken);
      setPortableStatus(ui("已导出 {p0} 文件。", { p0: String(format.toUpperCase()) }));
    } catch (error) { setPortableStatus(ui("导出失败：{p0}", { p0: String(uiError(error)) })); }
  }

  async function exportCurrentSection(format: NoteExportFormat) {
    const selected = await open({ directory: true, multiple: false });
    const directory = typeof selected === "string" ? selected : null;
    if (!directory) { setPortableStatus(ui("已取消导出。")); return; }
    const name = window.prompt(ui("导出目录名称"), `${(section as { name?: string } | null)?.name || "section"}-${format}`)?.trim();
    if (!name) { setPortableStatus(ui("已取消导出。")); return; }
    setPortableStatus(ui("正在导出分区…"));
    try {
      let confirmationToken: string | undefined;
      if (encryptedSection) {
        if (!window.confirm(ui("此导出会把整个加密分区写成明文文件。是否继续？"))) { setPortableStatus(ui("已取消导出。")); return; }
        confirmationToken = await api.requestSectionExportConfirmation(spaceId, sectionId, format, directory, name);
      }
      await api.exportSection(spaceId, sectionId, format, directory, name, false, confirmationToken);
      setPortableStatus(ui("已导出分区 {p0} 文件。", { p0: String(format.toUpperCase()) }));
    } catch (error) { setPortableStatus(ui("分区导出失败：{p0}", { p0: String(uiError(error)) })); }
  }

  function printCurrentPage() {
    if (!editor) return;
    const editable = editor.isEditable;
    const restore = () => {
      editor.setEditable(editable);
      window.removeEventListener("afterprint", restore);
    };
    editor.setEditable(false);
    window.addEventListener("afterprint", restore);
    window.print();
  }

  if (loadError) {
    return <div className="p-8 text-red-600">{ui("加载页面失败：")}{loadError}</div>;
  }

  if (locked || (sectionLocked && !page)) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-8">
        <span className="text-4xl">🔒</span>
        <p className="text-neutral-500">{ui("该分区已锁定，内容不可见")}</p>
        <UnlockDialogOpener spaceId={spaceId} sectionId={sectionId} />
      </div>
    );
  }

  if (!page || !editor) {
    return <div className="p-8 text-neutral-400">{ui("加载中…")}</div>;
  }

  const suggestItems = linkSuggest
    ? pageTitles.filter((t) => t.id !== pageId && t.title.toLowerCase().includes(linkSuggest.query.toLowerCase())).slice(0, 8)
    : [];

  return (
    <div className="flex h-full flex-col">
      {/* 标题 */}
      <div className="editor-heading flex flex-wrap items-center gap-2 border-b px-4 py-2">
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={commitTitle}
          placeholder={ui("页面标题")}
          disabled={sectionLocked}
          className="min-w-0 flex-1 bg-transparent text-xl font-bold outline-none"
        />
        {encryptedSection && <span title={ui("加密分区")}>{sectionLocked ? "🔒" : "🔓"}</span>}
        <details className="action-menu"><summary>{ui("更多")}</summary><div className="action-menu-panel">
        <button className="shrink-0 rounded border px-2 py-1 text-sm" onClick={() => setShowVersions((v) => !v)}>{ui("历史版本")}</button>
        <button className="shrink-0 rounded border px-2 py-1 text-sm" disabled={sectionLocked} onClick={importFiles}>{ui("导入")}</button>
        {(["markdown", "html", "pdf"] as NoteExportFormat[]).map((format) => (
          <button key={format} className="shrink-0 rounded border px-2 py-1 text-sm" onClick={() => exportCurrentPage(format)}>{ui("导出")}{format === "markdown" ? "MD" : format.toUpperCase()}</button>
        ))}
        <button className="shrink-0 rounded border px-2 py-1 text-sm" onClick={printCurrentPage}>{ui("打印")}</button>
        {(["markdown", "html", "pdf"] as NoteExportFormat[]).map((format) => (
          <button key={`section-${format}`} className="shrink-0 rounded border px-2 py-1 text-sm" onClick={() => exportCurrentSection(format)}>{ui("导出分区")}{format === "markdown" ? "MD" : format.toUpperCase()}</button>
        ))}
        </div></details>
      </div>

      {portableStatus && <div role="status" className="border-b bg-blue-50 px-4 py-2 text-sm text-blue-900 dark:bg-blue-950 dark:text-blue-100">{portableStatus}</div>}
      <details className="document-metadata"><summary>{ui("标签与页面工具")}</summary>
      {!sectionLocked && <TagSelector key={pageId} kind="page" id={pageId} spaceId={spaceId} />}
      {!sectionLocked && <SaveTemplateButton key={pageId} spaceId={spaceId} pageId={pageId} encrypted={!!encryptedSection} beforeSave={() => api.savePage(spaceId, pageId, title, JSON.stringify(editor.getJSON()))} />}
      {!sectionLocked && <NoteTasks key={pageId} editor={editor} spaceId={spaceId} pageId={pageId} beforeSave={async () => {
        if (saveTimer.current) clearTimeout(saveTimer.current);
        await api.savePage(spaceId, pageId, title, JSON.stringify(editor.getJSON()));
      }} refreshPage={async () => setPage(await api.getPage(spaceId, pageId))} />}
      </details>

      <div className="flex min-h-0 flex-1">
        <div className="relative min-w-0 flex-1 overflow-y-auto" data-print-document>
          <Toolbar editor={editor} onPickAttachment={pickAttachmentFile} disabled={sectionLocked} />
          <EditorContent editor={editor} className="mx-auto max-w-3xl px-6 py-4" />

          {/* [[ 页面选择弹层 */}
          {linkSuggest && suggestItems.length > 0 && (
            <div className="absolute left-8 top-20 z-30 w-72 rounded border bg-white shadow-lg dark:bg-neutral-800">
              {suggestItems.map((t) => (
                <button
                  key={t.id}
                  className="block w-full truncate px-3 py-1.5 text-left hover:bg-blue-50 dark:hover:bg-neutral-700"
                  onMouseDown={(e) => {
                    e.preventDefault();
                    applyPageLink(t);
                  }}
                >
                  {t.title}
                </button>
              ))}
            </div>
          )}
        </div>

        {showVersions && (
          <VersionsPanel spaceId={spaceId} pageId={pageId} current={page} onRollback={rollback} />
        )}
      </div>
    </div>
  );
}

function splitExportPath(path: string): { directory: string; filename: string } | null {
  const divider = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (divider <= 0 || divider === path.length - 1) return null;
  return { directory: path.slice(0, divider), filename: path.slice(divider + 1) };
}

function describeImportResult(result: BatchResult<ImportedPage>): string {
  const succeeded = result.items.filter((item) => item.status === "success").length;
  const skipped = result.items.filter((item) => item.status === "skipped").length;
  const failures = result.items.filter((item) => item.status === "failed");
  const reasons = failures.slice(0, 2).map((item) => `${item.item}：${item.reason}`).join("；");
  return ui("导入完成：{p0} 成功，{p1} 跳过，{p2} 失败{p3}", { p0: String(succeeded), p1: String(skipped), p2: String(failures.length), p3: String(reasons ? `（${reasons}）` : "") });
}

function UnlockDialogOpener({ spaceId, sectionId }: { spaceId: string; sectionId: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button className="rounded bg-blue-600 px-4 py-2 text-white" onClick={() => setOpen(true)}>{ui("解锁分区")}</button>
      {open && <UnlockDialog spaceId={spaceId} sectionId={sectionId} onClose={() => setOpen(false)} />}
    </>
  );
}

/** 版本历史面板（spec 7.1）。 */
function VersionsPanel({
  spaceId,
  pageId,
  current,
  onRollback,
}: {
  spaceId: string;
  pageId: string;
  current: Page;
  onRollback: (versionId: string) => void;
}) {
  const [versions, setVersions] = useState<PageVersion[] | null>(null);
  useEffect(() => {
    api.listVersions(spaceId, pageId).then(setVersions).catch(() => setVersions([]));
  }, [spaceId, pageId, current.updated_at]);

  return (
    <aside className="w-72 shrink-0 overflow-y-auto border-l p-3">
      <h4 className="mb-2 font-semibold">{ui("版本历史")}</h4>
      {!versions && <p className="text-sm text-neutral-400">{ui("加载中…")}</p>}
      {versions?.length === 0 && <p className="text-sm text-neutral-400">{ui("暂无历史版本")}</p>}
      <ul className="space-y-2">
        {versions?.map((v) => (
          <li key={v.id} className="rounded border p-2 text-sm">
            <div className="mb-1 flex items-center justify-between">
              <span className="text-neutral-500">{v.created_at}</span>
              <button
                className="rounded border px-2 py-0.5 text-xs"
                onClick={() => window.confirm(ui("回滚到该版本？当前内容会作为新版本保留。")) && onRollback(v.id)}
              >{ui("回滚")}</button>
            </div>
            <VersionPreview content={v.content} />
          </li>
        ))}
      </ul>
    </aside>
  );
}

function VersionPreview({ content }: { content: string }) {
  let text = content;
  try {
    const doc = JSON.parse(content);
    const parts: string[] = [];
    const walk = (n: any) => {
      if (n.text) parts.push(n.text);
      n.content?.forEach(walk);
    };
    walk(doc);
    text = parts.join(" ").slice(0, 120);
  } catch {
    text = content.slice(0, 120);
  }
  return <p className="line-clamp-3 text-neutral-600 dark:text-neutral-300">{text || ui("（空）")}</p>;
}

/** 格式工具栏（spec 5.2）。 */
function Toolbar({
  editor,
  onPickAttachment,
  disabled,
}: {
  editor: any;
  onPickAttachment: () => void;
  disabled: boolean;
}) {
  const btn = "rounded border px-2 py-1 text-sm disabled:opacity-40";
  const active = (name: string, attrs?: object) => editor?.isActive(name, attrs);
  const cls = (name: string, attrs?: object) => `${btn} ${active(name, attrs) ? "bg-blue-100 dark:bg-blue-900" : ""}`;
  const chain = () => editor?.chain().focus();

  return (
    <div className="flex flex-wrap items-center gap-1 border-b px-4 py-1.5">
      <select
        className={btn}
        disabled={disabled}
        value={
          active("heading", { level: 1 }) ? "1" : active("heading", { level: 2 }) ? "2" : active("heading", { level: 3 }) ? "3" : "0"
        }
        onChange={(e) => {
          const v = e.target.value;
          if (v === "0") chain().setParagraph().run();
          else chain().setHeading({ level: Number(v) as 1 | 2 | 3 }).run();
        }}
      >
        <option value="0">{ui("正文")}</option>
        <option value="1">{ui("标题 1")}</option>
        <option value="2">{ui("标题 2")}</option>
        <option value="3">{ui("标题 3")}</option>
      </select>
      <button className={cls("bold")} disabled={disabled} onClick={() => chain().toggleBold().run()}>
        <b>B</b>
      </button>
      <button className={cls("italic")} disabled={disabled} onClick={() => chain().toggleItalic().run()}>
        <i>I</i>
      </button>
      <button className={cls("underline")} disabled={disabled} onClick={() => chain().toggleUnderline().run()}>
        <u>U</u>
      </button>
      <button className={cls("strike")} disabled={disabled} onClick={() => chain().toggleStrike().run()}>
        <s>S</s>
      </button>
      <button className={cls("highlight")} disabled={disabled} onClick={() => chain().toggleHighlight().run()}>{ui("高亮")}</button>
      <label className={`${btn} flex items-center gap-1`} title={ui("字体颜色")}>
        <span>A</span>
        <input
          type="color"
          className="h-4 w-6"
          disabled={disabled}
          onChange={(e) => chain().setColor(e.target.value).run()}
        />
      </label>
      <select
        className={btn}
        disabled={disabled}
        defaultValue=""
        onChange={(e) => {
          if (e.target.value) chain().setFontSize(e.target.value).run();
        }}
      >
        <option value="">{ui("字号")}</option>
        {["12px", "14px", "16px", "18px", "22px", "28px"].map((s) => (
          <option key={s} value={s}>
            {s}
          </option>
        ))}
      </select>
      <button className={cls("bulletList")} disabled={disabled} onClick={() => chain().toggleBulletList().run()}>{ui("• 列表")}</button>
      <button className={cls("orderedList")} disabled={disabled} onClick={() => chain().toggleOrderedList().run()}>{ui("1. 列表")}</button>
      <button className={cls("taskList")} disabled={disabled} onClick={() => chain().toggleTaskList().run()}>{ui("☑ 待办")}</button>
      <button
        className={btn}
        disabled={disabled}
        onClick={() => chain().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run()}
      >{ui("表格")}</button>
      <button className={btn} disabled={disabled} onClick={() => chain().addColumnAfter().run()} title={ui("插入列")}>{ui("+列")}</button>
      <button className={btn} disabled={disabled} onClick={() => chain().addRowAfter().run()} title={ui("插入行")}>{ui("+行")}</button>
      <button className={btn} disabled={disabled} onClick={() => chain().deleteTable().run()} title={ui("删除表格")}>{ui("删表格")}</button>
      <button className={btn} disabled={disabled} onClick={() => chain().insertDrawingBlock().run()}>{ui("✏️ 绘图")}</button>
      <button className={btn} disabled={disabled} onClick={onPickAttachment}>{ui("📎 附件")}</button>
    </div>
  );
}
