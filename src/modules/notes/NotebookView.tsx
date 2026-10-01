import { Icon } from "../../shared/Icon";
import { ui, uiError } from "../../i18n/ui";
// 三栏导航骨架：空间/笔记本列表 → 分区/分区组树 → 页面树（spec/tasks 4.2）。
// 支持拖拽排序与移动（HTML5 DnD），选中态经 URL 深链（design D5）。

import { DragEvent, useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { api, PageNode, Section, SectionGroupNode } from "./api";
import { useNotesStore } from "./store";
import { usePrompt, SetPasswordDialog, UnlockDialog, ChangePasswordDialog, ConfirmDialog } from "./dialogs";
import EditorPage from "./EditorPage";
import { TemplateDialog } from "./Templates";

type DragPayload =
  | { kind: "notebook"; id: string }
  | { kind: "section"; id: string }
  | { kind: "group"; id: string }
  | { kind: "page"; id: string };

function setDrag(ev: DragEvent, payload: DragPayload) {
  ev.dataTransfer.setData("application/x-tenjee", JSON.stringify(payload));
  ev.dataTransfer.effectAllowed = "move";
}

function getDrag(ev: DragEvent): DragPayload | null {
  try {
    const raw = ev.dataTransfer.getData("application/x-tenjee");
    return raw ? (JSON.parse(raw) as DragPayload) : null;
  } catch {
    return null;
  }
}

export default function NotebookView() {
  const { spaceId = "", notebookId = "", sectionId = "", pageId = "" } = useParams();
  const navigate = useNavigate();
  const { tree, unlocked, settings, refreshTree } = useNotesStore();
  const [pageTrees, setPageTrees] = useState<Record<string, PageNode[]>>({});
  const { prompt, element } = usePrompt();
  const [dialog, setDialog] = useState<
    | { type: "set-password" | "unlock" | "change" | "remove"; sectionId: string }
    | null
  >(null);
  const [confirmDelete, setConfirmDelete] = useState<{ kind: string; id: string; name: string } | null>(null);
  const [templateTarget, setTemplateTarget] = useState<{ sectionId: string; parentPageId: string | null } | null>(null);

  const notebook = tree?.notebooks.find((n) => n.id === notebookId) ?? null;

  useEffect(() => {
    if (!sectionId) return;
    api
      .getPageTree(spaceId, sectionId)
      .then((nodes) => setPageTrees((m) => ({ ...m, [sectionId]: nodes })))
      .catch(() => {});
  }, [spaceId, sectionId, tree]);

  // ---- 创建 ----
  async function addNotebook() {
    const name = await prompt(ui("新建笔记本"), ui("名称"));
    if (!name?.trim()) return;
    await api.createNotebook(spaceId, name.trim(), null);
    await refreshTree();
  }

  async function addGroup(notebookId: string, parentGroupId: string | null) {
    const name = await prompt(ui("新建分区组"), ui("名称"));
    if (!name?.trim()) return;
    await api.createSectionGroup(spaceId, notebookId, parentGroupId, name.trim());
    await refreshTree();
  }

  async function addSection(notebookId: string, groupId: string | null) {
    const name = await prompt(ui("新建分区"), ui("名称"));
    if (!name?.trim()) return;
    const section = await api.createSection(spaceId, notebookId, groupId, name.trim(), null);
    await refreshTree();
    navigate(`/s/${spaceId}/nb/${notebookId}/sec/${section.id}`);
  }

  async function addPage(sectionId: string, parentPageId: string | null) {
    setTemplateTarget({ sectionId, parentPageId });
  }

  // ---- 删除 ----
  async function doDelete(kind: string, id: string) {
    if (kind === "notebook") await api.deleteNotebook(spaceId, id);
    else if (kind === "group") await api.deleteSectionGroup(spaceId, id);
    else if (kind === "section") await api.deleteSection(spaceId, id);
    else if (kind === "page") await api.deletePage(spaceId, id);
    await refreshTree();
    if (kind === "section") navigate(`/s/${spaceId}/nb/${notebookId}`);
    if (kind === "notebook") navigate(`/s/${spaceId}`);
  }

  // ---- 拖拽 ----
  async function onDropOnNotebook(ev: DragEvent, targetNotebookId: string, groupId: string | null) {
    const payload = getDrag(ev);
    if (!payload) return;
    ev.preventDefault();
    try {
      if (payload.kind === "section") {
        await api.moveSection(spaceId, payload.id, targetNotebookId, groupId);
      } else if (payload.kind === "group") {
        await api.moveSectionGroup(spaceId, payload.id, targetNotebookId, groupId);
      }
      await refreshTree();
    } catch (e) {
      alert(uiError(e));
    }
  }

  async function onDropOnPage(ev: DragEvent, target: PageNode, sectionId: string, index: number) {
    const payload = getDrag(ev);
    if (!payload || payload.kind !== "page" || payload.id === target.id) return;
    ev.preventDefault();
    try {
      await api.movePage(spaceId, payload.id, sectionId, target.parent_page_id, index);
      const nodes = await api.getPageTree(spaceId, sectionId);
      setPageTrees((m) => ({ ...m, [sectionId]: nodes }));
    } catch (e) {
      alert(uiError(e));
    }
  }

  const allowDrop = (ev: DragEvent) => ev.preventDefault();

  const lockedOf = (s: Section) => s.is_encrypted && !unlocked.includes(s.id);

  function sectionActions(s: Section) {
    return (
      <span className="hidden group-hover:inline">
        <button
          title={lockedOf(s) ? ui("解锁") : ui("锁定")}
          onClick={async (e) => {
            e.stopPropagation();
            if (lockedOf(s)) setDialog({ type: "unlock", sectionId: s.id });
            else {
              await api.lockSection(spaceId, s.id);
              await refreshTree();
            }
          }}
        >
          {lockedOf(s) ? "🔓" : "🔒"}
        </button>
        <button
          title={ui("更多")}
          onClick={(e) => {
            e.stopPropagation();
            const action = window.prompt(ui("操作：set=设置密码 change=修改密码 remove=移除密码 del=删除"), "");
            if (action === "set") setDialog({ type: "set-password", sectionId: s.id });
            else if (action === "change") setDialog({ type: "change", sectionId: s.id });
            else if (action === "remove") setDialog({ type: "remove", sectionId: s.id });
            else if (action === "del") setConfirmDelete({ kind: "section", id: s.id, name: s.name });
          }}
        >
          ⋯
        </button>
      </span>
    );
  }

  function renderSection(s: Section) {
    const active = s.id === sectionId;
    return (
      <div key={s.id}>
        <div
          draggable
          onDragStart={(e) => setDrag(e, { kind: "section", id: s.id })}
          onDrop={(e) => onDropOnNotebook(e, notebookId, s.section_group_id)}
          onDragOver={allowDrop}
          onClick={() => navigate(`/s/${spaceId}/nb/${notebookId}/sec/${s.id}`)}
          className={`group flex cursor-pointer items-center gap-1 rounded px-2 py-1 ${
            active ? "bg-blue-100 dark:bg-blue-900" : "hover:bg-neutral-100 dark:hover:bg-neutral-800"
          }`}
        >
          <span className="truncate">{s.name}</span>
          {s.is_encrypted && <span title={ui("加密分区")}>{lockedOf(s) ? "🔒" : "🔓"}</span>}
          <span className="ml-auto">{sectionActions(s)}</span>
        </div>
      </div>
    );
  }

  function renderGroup(g: SectionGroupNode, depth: number) {
    return (
      <div key={g.id} style={{ marginLeft: depth * 12 }}>
        <div
          draggable
          onDragStart={(e) => setDrag(e, { kind: "group", id: g.id })}
          onDrop={(e) => onDropOnNotebook(e, notebookId, g.id)}
          onDragOver={allowDrop}
          className="group flex items-center gap-1 rounded px-2 py-1 hover:bg-neutral-100 dark:hover:bg-neutral-800"
        >
          <span>📂 {g.name}</span>
          <span className="ml-auto hidden group-hover:inline">
            <button title={ui("新建子分区组")} onClick={() => addGroup(notebookId, g.id)}>{ui("＋组")}</button>
            <button title={ui("新建分区")} onClick={() => addSection(notebookId, g.id)}>{ui("＋区")}</button>
            <button title={ui("重命名")} onClick={async () => {
              const name = await prompt(ui("重命名分区组"), ui("名称"), g.name);
              if (name?.trim()) { await api.renameSectionGroup(spaceId, g.id, name.trim()); refreshTree(); }
            }}>✎</button>
            <button title={ui("删除")} onClick={() => setConfirmDelete({ kind: "group", id: g.id, name: g.name })}>🗑</button>
          </span>
        </div>
        <div className="ml-3 space-y-0.5">
          {g.sections.map(renderSection)}
          {g.children.map((child) => renderGroup(child, 1))}
        </div>
      </div>
    );
  }

  function renderPageNode(node: PageNode, sectionId: string, depth: number, index: number) {
    const showLockedTitles = settings?.encrypted_section_show_titles ?? true;
    return (
      <div key={node.id}>
        <div
          draggable
          onDragStart={(e) => setDrag(e, { kind: "page", id: node.id })}
          onDrop={(e) => onDropOnPage(e, node, sectionId, index)}
          onDragOver={allowDrop}
          onClick={() => navigate(`/s/${spaceId}/nb/${notebookId}/sec/${sectionId}/page/${node.id}`)}
          className={`group flex cursor-pointer items-center gap-1 rounded px-2 py-1 ${node.id === pageId ? "bg-blue-100 dark:bg-blue-900" : "hover:bg-neutral-100 dark:hover:bg-neutral-800"}`}
          style={{ marginLeft: depth * 14 }}
        >
          <span className="truncate">{node.title || ui("（无标题）")}</span>
          <span className="ml-auto hidden gap-1 group-hover:inline">
            <button
              title={ui("新建子页面")}
              onClick={(e) => {
                e.stopPropagation();
                addPage(sectionId, node.id);
              }}
            >
              ＋
            </button>
            <button
              title={ui("重命名")}
              onClick={async (e) => {
                e.stopPropagation();
                const title = await prompt(ui("重命名页面"), ui("标题"), node.title);
                if (title !== null) {
                  await api.renamePage(spaceId, node.id, title.trim());
                  const nodes = await api.getPageTree(spaceId, sectionId);
                  setPageTrees((m) => ({ ...m, [sectionId]: nodes }));
                }
              }}
            >
              ✎
            </button>
            <button
              title={ui("删除（进回收站）")}
              onClick={(e) => {
                e.stopPropagation();
                setConfirmDelete({ kind: "page", id: node.id, name: node.title });
              }}
            >
              🗑
            </button>
          </span>
        </div>
        {/* 锁定分区按设置隐藏页面标题（spec 6.4） */}
        {(!lockedOf((notebook?.sections ?? []).find((s) => s.id === sectionId)!) || showLockedTitles) &&
          node.children.map((child, i) => renderPageNode(child, sectionId, depth + 1, i))}
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1">
      {/* 栏 2：分区/分区组树 */}
      {notebook && <aside className="notes-sections w-60 shrink-0 space-y-2 overflow-y-auto border-r p-2">
        <div className="flex items-center justify-between">
          <h3 className="font-semibold">{notebook?.name ?? ui("选择笔记本")}</h3>
          {notebook && (
            <details className="action-menu"><summary aria-label={ui("更多")}>⋯</summary><div className="action-menu-panel">
              <button title={ui("新建分区组")} onClick={() => addGroup(notebookId, null)}>{ui("＋组")}</button>
              <button title={ui("新建分区")} onClick={() => addSection(notebookId, null)}>{ui("＋区")}</button>
              <button
                title={ui("重命名笔记本")}
                onClick={async () => {
                  const name = await prompt(ui("重命名笔记本"), ui("名称"), notebook.name);
                  if (name?.trim()) { await api.renameNotebook(spaceId, notebookId, name.trim()); refreshTree(); }
                }}
              >
                ✎
              </button>
              <button title={ui("删除笔记本")} onClick={() => setConfirmDelete({ kind: "notebook", id: notebookId, name: notebook.name })}>🗑</button>
            </div></details>
          )}
        </div>
        {!notebook && <p className="text-sm text-neutral-400">{ui("从左侧选择一个笔记本")}</p>}
        {notebook && (
          <div className="space-y-0.5">
            {notebook.sections.map(renderSection)}
            {notebook.groups.map((g) => renderGroup(g, 0))}
          </div>
        )}
        {notebook && (
          <button className="mt-2 rounded border px-2 py-1 text-sm" onClick={addNotebook}>{ui("＋ 新建笔记本")}</button>
        )}
      </aside>}

      {/* 栏 3：页面树 */}
      {sectionId && <aside className="notes-pages w-60 shrink-0 overflow-y-auto border-r p-2">
        <div className="flex items-center justify-between">
          <h3 className="font-semibold">{ui("页面")}</h3>
          {sectionId && (
            <button className="text-sm" onClick={() => addPage(sectionId, null)}>{ui("＋页")}</button>
          )}
        </div>
        {!sectionId && <p className="mt-2 text-sm text-neutral-400">{ui("选择一个分区查看页面")}</p>}
        {sectionId && (pageTrees[sectionId] ?? []).map((node, i) => renderPageNode(node, sectionId, 0, i))}
        {sectionId && (pageTrees[sectionId] ?? []).length === 0 && (
          <p className="mt-2 text-sm text-neutral-400">{ui("暂无页面，点击 ＋页 创建")}</p>
        )}
      </aside>}

      {/* 主区：编辑器 */}
      <section className="flex min-w-0 flex-1 flex-col">
        {pageId ? (
          <EditorPage key={`${spaceId}:${pageId}`} />
        ) : (
          <div className="notes-welcome">
            <div className="welcome-symbol"><Icon name="notes" size={32} /></div>
            <p className="welcome-eyebrow">TENJEE VAULT</p>
            <h1>{ui("让想法有处安放")}</h1>
            <p className="welcome-description">{ui("在这里记录想法、整理计划，让重要的事井井有条。")}</p>
            {notebook ? <><p className="welcome-hint">{ui(sectionId ? "选择或新建一个页面开始书写" : "选择一个分区查看页面")}</p><button className="primary-button" onClick={() => sectionId ? addPage(sectionId, null) : addSection(notebookId, null)}><Icon name="plus" />{ui(sectionId ? "＋页" : "新建分区")}</button></> : <div className="welcome-notebooks">
              <div className="welcome-list-heading"><span>{ui("你的笔记本")}</span><button onClick={addNotebook}><Icon name="plus" size={16} />{ui("新建笔记本")}</button></div>
              {(tree?.notebooks ?? []).map((item) => <button key={item.id} className="notebook-card" onClick={() => navigate(`/s/${spaceId}/nb/${item.id}`)}><span className="notebook-icon" style={{ color: item.color ?? "var(--color-accent)" }}><Icon name="notes" size={22} /></span><span>{item.name}</span><span className="notebook-arrow">→</span></button>)}
              <p className="welcome-hint">{ui(tree?.notebooks.length ? "选择笔记本，继续你的记录。" : "创建第一个笔记本，开始你的记录。")}</p>
              {!tree?.notebooks.length && <button className="primary-button" disabled={!tree} onClick={addNotebook}><Icon name="plus" />{ui("新建笔记本")}</button>}
            </div>}
          </div>
        )}
      </section>

      {/* 对话框 */}
      {element}
      {templateTarget && <TemplateDialog spaceId={spaceId} sectionId={templateTarget.sectionId} parentPageId={templateTarget.parentPageId} onClose={() => setTemplateTarget(null)} onCreated={async (page) => {
        setTemplateTarget(null); await refreshTree();
        navigate(`/s/${spaceId}/nb/${notebookId}/sec/${page.section_id}/page/${page.id}`);
      }} />}
      {dialog?.type === "set-password" && (
        <SetPasswordDialog spaceId={spaceId} sectionId={dialog.sectionId} onClose={() => setDialog(null)} />
      )}
      {dialog?.type === "unlock" && (
        <UnlockDialog spaceId={spaceId} sectionId={dialog.sectionId} onClose={() => setDialog(null)} />
      )}
      {dialog?.type === "change" && (
        <ChangePasswordDialog spaceId={spaceId} sectionId={dialog.sectionId} mode="change" onClose={() => setDialog(null)} />
      )}
      {dialog?.type === "remove" && (
        <ChangePasswordDialog spaceId={spaceId} sectionId={dialog.sectionId} mode="remove" onClose={() => setDialog(null)} />
      )}
      {confirmDelete && (
        <ConfirmDialog
          title={ui("确认删除")}
          danger
          message={ui("删除{p0}「{p1}」？{p2}", { p0: String({ notebook: ui("笔记本"), group: ui("分区组"), section: ui("分区"), page: ui("页面") }[confirmDelete.kind]), p1: String(confirmDelete.name), p2: String(confirmDelete.kind === "page"
              ? ui("页面将进入回收站，可恢复。")
              : ui("其中页面将进入回收站（容器本身不可恢复）。")) })}
          confirmText={ui("删除")}
          onConfirm={() => doDelete(confirmDelete.kind, confirmDelete.id)}
          onClose={() => setConfirmDelete(null)}
        />
      )}
    </div>
  );
}
