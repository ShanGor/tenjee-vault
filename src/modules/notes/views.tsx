import { ui } from "../../i18n/ui";
// 回收站（4.3）、最近使用（4.3）、全局搜索（7.2）视图。

import { FormEvent, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, RecentEntry, SearchResult, TrashEntry } from "./api";
import { useNotesStore } from "./store";
import { ConfirmDialog } from "./dialogs";

export function TrashView() {
  const spaceId = useNotesStore((s) => s.currentSpaceId) ?? "";
  const [items, setItems] = useState<TrashEntry[]>([]);
  const [purge, setPurge] = useState<TrashEntry | null>(null);

  const reload = () => api.listTrash(spaceId).then(setItems).catch(() => {});
  useEffect(() => {
    reload();
  }, [spaceId]);

  async function restore(id: string) {
    await api.restorePage(spaceId, id);
    await reload();
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto p-4">
      <h2 className="mb-3 text-lg font-semibold">{ui("回收站")}</h2>
      {items.length === 0 && <p className="text-neutral-400">{ui("回收站为空")}</p>}
      <ul className="max-w-2xl space-y-2">
        {items.map((t) => (
          <li key={t.id} className="flex items-center gap-3 rounded border px-3 py-2">
            <div className="min-w-0 flex-1">
              <div className="truncate">{t.title || ui("（无标题）")}</div>
              <div className="text-xs text-neutral-400">
                {t.notebook_name} / {t.section_name} · {t.deleted_at}
              </div>
            </div>
            <button className="rounded border px-2 py-1 text-sm" onClick={() => restore(t.id)}>{ui("恢复")}</button>
            <button className="rounded border px-2 py-1 text-sm text-red-600" onClick={() => setPurge(t)}>{ui("彻底删除")}</button>
          </li>
        ))}
      </ul>
      {purge && (
        <ConfirmDialog
          title={ui("彻底删除")}
          danger
          message={ui("彻底删除「{p0}」？页面及其版本数据将被物理移除，不可恢复。", { p0: String(purge.title) })}
          confirmText={ui("彻底删除")}
          onConfirm={async () => {
            await api.purgePage(spaceId, purge.id);
            await reload();
          }}
          onClose={() => setPurge(null)}
        />
      )}
    </div>
  );
}

export function RecentView() {
  const navigate = useNavigate();
  const [items, setItems] = useState<RecentEntry[]>([]);
  useEffect(() => {
    api.listRecent(50).then(setItems).catch(() => {});
  }, []);

  return (
    <div className="min-h-0 flex-1 overflow-y-auto p-4">
      <h2 className="mb-3 text-lg font-semibold">{ui("最近使用")}</h2>
      {items.length === 0 && <p className="text-neutral-400">{ui("暂无最近使用的页面")}</p>}
      <ul className="max-w-2xl space-y-1">
        {items.map((r) => (
          <li key={`${r.space_id}:${r.page_id}`}>
            <button
              className="block w-full truncate rounded border px-3 py-2 text-left hover:bg-neutral-50 dark:hover:bg-neutral-800"
              onClick={() => navigate(`/s/${r.space_id}/page/${r.page_id}`)}
            >
              {r.title} <span className="ml-2 text-xs text-neutral-400">{r.opened_at}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function SearchView() {
  const spaceId = useNotesStore((s) => s.currentSpaceId) ?? "";
  const navigate = useNavigate();
  const tree = useNotesStore((s) => s.tree);
  const [query, setQuery] = useState("");
  const [notebookId, setNotebookId] = useState("");
  const [sectionId, setSectionId] = useState("");
  const [results, setResults] = useState<SearchResult[] | null>(null);
  const [elapsed, setElapsed] = useState<number | null>(null);

  const sections = useMemo(() => {
    const out: { id: string; name: string }[] = [];
    for (const nb of tree?.notebooks ?? []) {
      for (const s of nb.sections) out.push({ id: s.id, name: `${nb.name} / ${s.name}` });
      const walk = (g: { name: string; sections: { id: string; name: string }[]; children: never[] }) => {
        for (const s of g.sections) out.push({ id: s.id, name: `${nb.name} / ${g.name} / ${s.name}` });
        g.children.forEach(walk);
      };
      nb.groups.forEach(walk as never);
    }
    return out;
  }, [tree]);

  async function runSearch(e: FormEvent) {
    e.preventDefault();
    if (!query.trim()) return;
    const start = performance.now();
    const hits = await api.searchNotes(
      spaceId,
      query.trim(),
      notebookId || null,
      sectionId || null,
      100,
    );
    setResults(hits);
    setElapsed(performance.now() - start);
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto p-4">
      <h2 className="mb-3 text-lg font-semibold">{ui("搜索")}</h2>
      <form onSubmit={runSearch} className="mb-4 flex max-w-3xl gap-2">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={ui("输入关键词，回车搜索（支持中文）")}
          className="flex-1 rounded border px-3 py-1.5 dark:bg-neutral-900"
        />
        <select
          value={notebookId}
          onChange={(e) => setNotebookId(e.target.value)}
          className="rounded border px-2 py-1.5 text-sm dark:bg-neutral-900"
        >
          <option value="">{ui("全部笔记本")}</option>
          {(tree?.notebooks ?? []).map((nb) => (
            <option key={nb.id} value={nb.id}>
              {nb.name}
            </option>
          ))}
        </select>
        <select
          value={sectionId}
          onChange={(e) => setSectionId(e.target.value)}
          className="rounded border px-2 py-1.5 text-sm dark:bg-neutral-900"
        >
          <option value="">{ui("全部分区")}</option>
          {sections.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
        <button type="submit" className="rounded bg-blue-600 px-4 py-1.5 text-white">{ui("搜索")}</button>
      </form>

      {results && (
        <p className="mb-2 text-sm text-neutral-400">
          {results.length}{ui("条结果")}{elapsed !== null && ` · ${elapsed.toFixed(0)}ms`}
          {results.some((r) => r.from_unlocked) && ui("（含已解锁加密分区）")}
        </p>
      )}
      <ul className="max-w-3xl space-y-2">
        {results?.map((r) => {
          const nb = tree?.notebooks.find((n) => n.sections.some((s) => s.id === r.section_id));
          return (
            <li key={r.page_id}>
              <button
                className="block w-full rounded border px-3 py-2 text-left hover:bg-neutral-50 dark:hover:bg-neutral-800"
                onClick={() => navigate(`/s/${spaceId}/page/${r.page_id}`)}
              >
                <div className="flex items-baseline gap-2">
                  <span className="font-medium">{r.title}</span>
                  <span className="text-xs text-neutral-400">
                    {r.notebook_name} / {r.section_name}
                    {r.from_unlocked && " 🔓"}
                  </span>
                </div>
                <div
                  className="mt-0.5 text-sm text-neutral-600 dark:text-neutral-300 [&_mark]:bg-yellow-200 dark:[&_mark]:bg-yellow-700"
                  dangerouslySetInnerHTML={{ __html: r.snippet }}
                />
                {nb && <span className="hidden">{nb.id}</span>}
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
