import { Icon } from "../../shared/Icon";
import { ui } from "../../i18n/ui";
// 应用外壳：空间侧栏（栏 1）、心跳巡检、锁定事件同步（spec 6.2）。

import { useEffect, useState } from "react";
import { NavLink, Outlet, useNavigate, useParams } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { useNotesStore } from "./store";
import { usePrompt, ConfirmDialog } from "./dialogs";

export default function AppShell() {
  const navigate = useNavigate();
  const { spaceId = "" } = useParams();
  const { spaces, tree, currentSpaceId, settings, refreshSpaces, selectSpace, loadSettings, refreshTree, removeUnlocked } =
    useNotesStore();
  const { prompt, element } = usePrompt();
  const [confirm, setConfirm] = useState<{ kind: "archive" | "delete"; id: string; name: string } | null>(null);

  // 初始化：设置 → 空间 → 默认空间兜底 → 选中第一个空间
  useEffect(() => {
    (async () => {
      await loadSettings();
      let list = await api.listSpaces();
      if (list.length === 0) list = await api.ensureDefaultSpace();
      useNotesStore.setState({ spaces: list });
      const target = spaceId || list[0]?.id;
      if (target) {
        if (!spaceId) navigate(`/s/${target}`, { replace: true });
        if (target !== currentSpaceId) await selectSpace(target);
      }
    })().catch((e) => console.error(ui("初始化失败"), e));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 路由中的空间变化
  useEffect(() => {
    if (spaceId && spaceId !== currentSpaceId) {
      selectSpace(spaceId).catch((e) => console.error(e));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [spaceId]);

  // 活动心跳（30s）+ 锁定事件：同步导航/编辑器/搜索（spec 6.2）
  useEffect(() => {
    const beat = async () => {
      try {
        const locked = await api.activityHeartbeat();
        for (const id of locked) removeUnlocked(id);
        if (locked.length > 0) await refreshTree();
      } catch {
        /* 后端不可达时静默，下轮重试 */
      }
    };
    const timer = setInterval(beat, 30_000);
    const unlisten = listen<{ section_id: string }>("section-locked", async (event) => {
      removeUnlocked(event.payload.section_id);
      await refreshTree();
    });
    return () => {
      clearInterval(timer);
      unlisten.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function onSpaceChange(id: string) {
    navigate(`/s/${id}`);
  }

  async function addSpace() {
    const name = await prompt(ui("新建空间"), ui("空间名称"));
    if (!name?.trim()) return;
    await api.createSpace(name.trim());
    const list = await refreshSpaces();
    if (list.length > 0) navigate(`/s/${list[list.length - 1].id}`);
  }

  async function lockAll() {
    await api.lockAllSections();
    await refreshTree();
  }

  const linkCls = ({ isActive }: { isActive: boolean }) =>
    `sidebar-link flex items-center gap-2 rounded px-2 py-1 text-sm ${isActive ? "bg-blue-100 dark:bg-blue-900" : "hover:bg-neutral-100 dark:hover:bg-neutral-800"}`;

  return (
    <div className="flex h-full flex-col text-neutral-900 dark:text-neutral-100">
      <header className="workspace-toolbar flex items-center gap-3 border-b px-4 py-2">
        <span className="workspace-label">{ui("空间")}</span>
        <select
          aria-label={ui("空间")}
          className="rounded border px-2 py-1 text-sm dark:bg-neutral-900"
          value={spaceId}
          onChange={(e) => onSpaceChange(e.target.value)}
        >
          {spaces.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
        <button className="rounded border px-2 py-1 text-sm" onClick={addSpace}>{ui("＋空间")}</button>
        {spaceId && (
          <details className="action-menu"><summary aria-label={ui("更多")}>⋯</summary><div className="action-menu-panel">
            <button
              className="rounded border px-2 py-1 text-sm"
              title={ui("重命名空间")}
              onClick={async () => {
                const name = await prompt(ui("重命名空间"), ui("名称"), spaces.find((s) => s.id === spaceId)?.name);
                if (name?.trim()) {
                  await api.renameSpace(spaceId, name.trim());
                  await refreshSpaces();
                }
              }}
            >
              ✎
            </button>
            <button
              className="rounded border px-2 py-1 text-sm"
              title={ui("归档空间（数据保留）")}
              onClick={() => {
                const s = spaces.find((x) => x.id === spaceId);
                if (s) setConfirm({ kind: "archive", id: s.id, name: s.name });
              }}
            >{ui("归档")}</button>
            <button
              className="rounded border px-2 py-1 text-sm text-red-600"
              title={ui("删除空间（库文件与附件一并移除）")}
              onClick={() => {
                const s = spaces.find((x) => x.id === spaceId);
                if (s) setConfirm({ kind: "delete", id: s.id, name: s.name });
              }}
            >{ui("删除")}</button>
          </div></details>
        )}
        <span className="ml-auto flex items-center gap-2 text-sm">
          {settings && (
            <label className="flex items-center gap-1 text-neutral-500">{ui("闲置")}{settings.section_auto_lock_minutes}{ui("分钟自动锁定")}</label>
          )}
          <button className="rounded border px-2 py-1" onClick={lockAll} title={ui("锁定全部加密分区")}><span className="flex items-center gap-2"><Icon name="lock" size={15} />{ui("全部锁定")}</span></button>
        </span>
      </header>

      <div className="flex min-h-0 flex-1">
        {/* 栏 1：空间内笔记本列表 + 全局视图入口 */}
        <aside className="flex w-52 shrink-0 flex-col border-r">
          <nav className="space-y-0.5 p-2">
            <NavLink to="/recent" className={linkCls}><Icon name="recent" />{ui("最近使用")}</NavLink>
            <NavLink to="/search" className={linkCls}><Icon name="search" />{ui("搜索")}</NavLink>
            <NavLink to="/trash" className={linkCls}><Icon name="trash" />{ui("回收站")}</NavLink>
          </nav>
          <div className="min-h-0 flex-1 overflow-y-auto border-t p-2">
            <h3 className="mb-1 px-2 text-xs font-semibold uppercase text-neutral-400">{ui("笔记本")}</h3>
            {(tree?.notebooks ?? []).map((nb) => (
              <NavLink
                key={nb.id}
                to={`/s/${spaceId}/nb/${nb.id}`}
                className={linkCls}
              >
                <span className="mr-1 inline-block h-2 w-2 rounded-full" style={{ background: nb.color ?? "#94a3b8" }} />
                {nb.name}
              </NavLink>
            ))}
            {tree && tree.notebooks.length === 0 && (
              <p className="px-2 text-sm text-neutral-400">{ui("暂无笔记本，去右侧创建")}</p>
            )}
          </div>
        </aside>

        <main className="flex min-w-0 flex-1 flex-col">
          <Outlet />
        </main>
      </div>

      {element}
      {confirm && (
        <ConfirmDialog
          title={confirm.kind === "archive" ? ui("归档空间") : ui("删除空间")}
          danger={confirm.kind === "delete"}
          message={
            confirm.kind === "archive"
              ? ui("归档「{p0}」？空间将从导航消失，数据保留、可恢复。", { p0: String(confirm.name) })
              : ui("删除「{p0}」？该空间的库文件与附件目录将一并移除，不可恢复。", { p0: String(confirm.name) })
          }
          confirmText={confirm.kind === "archive" ? ui("归档") : ui("永久删除")}
          onConfirm={async () => {
            if (confirm.kind === "archive") await api.archiveSpace(confirm.id);
            else await api.deleteSpace(confirm.id);
            const list = await refreshSpaces();
            if (list.length > 0) navigate(`/s/${list[0].id}`);
            else navigate("/", { replace: true });
          }}
          onClose={() => setConfirm(null)}
        />
      )}
    </div>
  );
}
