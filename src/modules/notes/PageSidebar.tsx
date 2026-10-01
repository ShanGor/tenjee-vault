import { usePrompt } from "./usePrompt";
import { DragEvent, KeyboardEvent, useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { Icon } from "../../shared/Icon";
import { ui, uiError } from "../../i18n/ui";
import { api, SpacePageNode } from "./api";
import { useNotesStore } from "./store";
import { flushPageSave } from "./pageSave";
import { pagePath } from "./pageTree";
import { TemplateDialog } from "./Templates";
import { ConfirmDialog, SetPasswordDialog, UnlockDialog, ChangePasswordDialog } from "./dialogs";
import { readViewState, writeViewState } from "../../shared/viewState";

export default function PageSidebar() {
  const { pageId = "" } = useParams();
  const navigate = useNavigate();
  const { currentSpaceId, tree, unlocked, settings, loading, refreshTree } = useNotesStore();
  const spaceId = currentSpaceId ?? "";
  const [expandedBySpace, setExpandedBySpace] = useState<Record<string, string[]>>(() => readViewState("notes-expanded", {}));
  const expanded = new Set(expandedBySpace[spaceId] ?? []);
  function setExpanded(update: (previous: Set<string>) => Set<string>) {
    const next = { ...expandedBySpace, [spaceId]: [...update(new Set(expandedBySpace[spaceId] ?? []))] };
    // Write in the event/effect itself so an immediate module switch cannot unmount
    // the sidebar before a deferred React updater reaches storage.
    writeViewState("notes-expanded", next);
    setExpandedBySpace(next);
  }
  const [target, setTarget] = useState<{ parent: string | null; domain: string } | null>(null);
  const [dialog, setDialog] = useState<{ kind: "protect" | "unlock" | "change" | "remove"; node: SpacePageNode } | null>(null);
  const [deleting, setDeleting] = useState<SpacePageNode | null>(null);
  const [error, setError] = useState("");
  const { prompt, element } = usePrompt();
  const nodes = tree?.pages ?? [];
  const locked = (node: SpacePageNode) => node.is_encrypted && !unlocked.includes(node.section_id);

  function visiblePages(items: SpacePageNode[]): SpacePageNode[] {
    return items.flatMap((node) => [node, ...(expanded.has(node.id) && !(locked(node) && settings?.encrypted_section_show_titles === false) ? visiblePages(node.children) : [])]);
  }

  function handleTreeKeyDown(event: KeyboardEvent<HTMLDivElement>, node: SpacePageNode) {
    if (event.altKey || event.ctrlKey || event.metaKey || !["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
    if (!(event.target instanceof Element) || !event.target.closest(".page-tree-link, .page-tree-toggle")) return;
    const pages = visiblePages(nodes);
    const index = pages.findIndex((page) => page.id === node.id);
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      if (event.key === "ArrowRight" && node.children.length && !expanded.has(node.id) && !(locked(node) && settings?.encrypted_section_show_titles === false)) {
        setExpanded((previous) => new Set([...previous, node.id]));
      } else if (event.key === "ArrowLeft" && expanded.has(node.id)) {
        setExpanded((previous) => { const next = new Set(previous); next.delete(node.id); return next; });
      }
      return;
    }

    const next = pages[index + (event.key === "ArrowDown" ? 1 : -1)];
    if (!next) return;
    event.preventDefault();
    const link = [...document.querySelectorAll<HTMLButtonElement>(".page-tree-link")].find((candidate) => candidate.dataset.pageId === next.id);
    link?.focus();
    void act(async () => navigate(`/s/${spaceId}/page/${encodeURIComponent(next.id)}`));
  }

  useEffect(() => { setTarget(null); setDialog(null); setDeleting(null); setError(""); }, [spaceId]);
  useEffect(() => {
    const path = pagePath(nodes, pageId);
    setExpanded((previous) => new Set([...previous, ...path.slice(0, -1).map((node) => node.id)]));
  }, [tree, pageId]);

  async function act(action: () => Promise<unknown>) {
    setError("");
    try { await flushPageSave(); await action(); } catch (error) { setError(uiError(error)); }
  }

  function add(parent: SpacePageNode | null) {
    if (parent && locked(parent)) { setDialog({ kind: "unlock", node: parent }); return; }
    setTarget({ parent: parent?.id ?? null, domain: parent?.is_encrypted ? parent.section_id : "__plain_pages__" });
  }

  async function move(event: DragEvent, parent: string | null, order: number) {
    event.preventDefault(); event.stopPropagation();
    const id = event.dataTransfer.getData("application/x-tenjee-page");
    if (!id) return;
    await act(async () => {
      await api.movePage(spaceId, id, parent, order);
      if (parent) setExpanded((previous) => new Set([...previous, parent]));
      await refreshTree();
    });
  }

  function render(node: SpacePageNode, depth: number, index: number) {
    const isLocked = locked(node);
    const open = expanded.has(node.id);
    const title = isLocked && settings?.encrypted_section_show_titles === false ? ui("受保护页面") : node.title || ui("（无标题）");
    return <div key={node.id}>
      <div className="page-drop-target" onDragOver={(event) => event.preventDefault()} onDrop={(event) => void move(event, node.parent_page_id, index)} />
      <div className={`page-tree-row ${node.id === pageId ? "active" : ""}`} style={{ paddingLeft: 6 + depth * 14 }} draggable={!isLocked} onKeyDown={(event) => handleTreeKeyDown(event, node)}
        onDragStart={(event) => { event.stopPropagation(); event.dataTransfer.setData("application/x-tenjee-page", node.id); event.dataTransfer.effectAllowed = "move"; }}
        onDragOver={(event) => { event.preventDefault(); event.stopPropagation(); }} onDrop={(event) => void move(event, node.id, node.children.length)}>
        <button className="page-tree-toggle" aria-label={ui(open ? "收起子页面" : "展开子页面")} aria-expanded={open} disabled={!node.children.length || (isLocked && settings?.encrypted_section_show_titles === false)} onClick={() => setExpanded((previous) => { const next = new Set(previous); if (open) next.delete(node.id); else next.add(node.id); return next; })}>{node.children.length ? open ? "▾" : "▸" : "·"}</button>
        <button className="page-tree-link" data-page-id={node.id} title={title} onClick={() => void act(async () => navigate(`/s/${spaceId}/page/${encodeURIComponent(node.id)}`))}>
          <Icon name={isLocked ? "lock" : "notes"} size={15} /><span>{title}</span>
        </button>
        <details className="action-menu page-tree-menu"><summary aria-label={ui("页面操作")}>⋯</summary><div className="action-menu-panel" onClick={(event) => { const details = event.currentTarget.parentElement as HTMLDetailsElement; details.open = false; }}>
          <button disabled={isLocked} onClick={() => add(node)}>{ui("新建子页面")}</button>
          <button disabled={isLocked} onClick={() => void act(async () => { const name = await prompt(ui("重命名页面"), ui("标题"), node.title); if (name?.trim()) { await api.renamePage(spaceId, node.id, name.trim()); await refreshTree(); } })}>{ui("重命名")}</button>
          <button disabled={isLocked} onClick={() => void act(async () => { await api.movePage(spaceId, node.id, null, nodes.length); await refreshTree(); })}>{ui("移到空间顶层")}</button>
          {node.is_encrypted ? <>
            <button onClick={() => isLocked ? setDialog({ kind: "unlock", node }) : void act(async () => { await api.lockSection(spaceId, node.section_id); await refreshTree(); })}>{ui(isLocked ? "解锁" : "锁定")}</button>
            {node.protection_root_id === node.id && <>
              <button onClick={() => setDialog({ kind: "change", node })}>{ui("修改密码")}</button>
              <button onClick={() => setDialog({ kind: "remove", node })}>{ui("移除密码")}</button>
            </>}
          </> : <button onClick={() => setDialog({ kind: "protect", node })}>{ui("保护页面及子页面")}</button>}
          <button disabled={isLocked} onClick={() => setDeleting(node)}>{ui("删除（进回收站）")}</button>
        </div></details>
      </div>
      {open && !(isLocked && settings?.encrypted_section_show_titles === false) && node.children.map((child, index) => render(child, depth + 1, index))}
    </div>;
  }

  return <div className="page-sidebar min-h-0 flex-1 overflow-y-auto border-t p-2">
    <div className="mb-2 flex items-center justify-between px-2"><h3 className="text-xs font-semibold uppercase text-neutral-400">{ui("页面")}</h3></div>
    {nodes.map((node, index) => render(node, 0, index))}
    {!nodes.length && <p className="px-2 text-sm text-neutral-400">{ui(loading ? "加载中…" : "暂无页面，创建第一个页面")}</p>}
    <div className="page-root-drop" onDragOver={(event) => event.preventDefault()} onDrop={(event) => void move(event, null, nodes.length)}>
      <button className="mt-2 flex w-full items-center gap-2 rounded border px-2 py-1.5 text-sm" disabled={!tree || loading || !spaceId} onClick={() => add(null)}><Icon name="plus" size={15} />{ui("新建页面")}</button>
    </div>
    {error && <p role="alert" className="mt-2 px-2 text-sm text-red-600">{error}</p>}
    {element}
    {target && <TemplateDialog spaceId={spaceId} sectionId={target.domain} parentPageId={target.parent} onClose={() => setTarget(null)} onCreated={async (page) => { setTarget(null); await refreshTree(); navigate(`/s/${spaceId}/page/${page.id}`); }} />}
    {dialog?.kind === "protect" && <SetPasswordDialog spaceId={spaceId} pageId={dialog.node.id} onClose={() => setDialog(null)} />}
    {dialog?.kind === "unlock" && <UnlockDialog spaceId={spaceId} sectionId={dialog.node.section_id} onClose={() => setDialog(null)} />}
    {(dialog?.kind === "change" || dialog?.kind === "remove") && <ChangePasswordDialog spaceId={spaceId} sectionId={dialog.node.section_id} mode={dialog.kind} onClose={() => setDialog(null)} />}
    {deleting && <ConfirmDialog title={ui("确认删除")} danger message={ui("删除页面「{p0}」及其子页面？可以从回收站恢复。", { p0: deleting.title })} confirmText={ui("删除")} onClose={() => setDeleting(null)} onConfirm={async () => {
      await flushPageSave();
      await api.deletePage(spaceId, deleting.id);
      if (pagePath(nodes, pageId).some((node) => node.id === deleting.id)) navigate(`/s/${spaceId}`);
      await refreshTree();
    }} />}
  </div>;
}
