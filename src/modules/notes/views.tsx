import { ui } from "../../i18n/ui";
// 回收站（4.3）、最近使用（4.3）、全局搜索（7.2）视图。

import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, RecentEntry, SearchResult, TrashEntry } from "./api";
import { flattenPages } from "./pageTree";
import { useNotesStore } from "./store";
import { ConfirmDialog } from "./dialogs";

export function TrashView() {
  const spaceId = useNotesStore((s) => s.currentSpaceId) ?? "";
  const [items, setItems] = useState<TrashEntry[]>([]);
  const [purge, setPurge] = useState<TrashEntry | null>(null);
  const unlocked = useNotesStore((s) => s.unlocked);

  const reload = () => api.listTrash(spaceId).then(setItems).catch(() => {});
  useEffect(() => {
    let cancelled = false;
    setItems([]); setPurge(null);
    api.listTrash(spaceId).then((next) => { if (!cancelled) setItems(next); }).catch(() => {});
    return () => { cancelled = true; };
  }, [spaceId, unlocked]);

  async function restore(id: string) {
    await api.restorePage(spaceId, id);
    await reload();
    await useNotesStore.getState().refreshTree();
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
                {t.path} · {t.deleted_at}
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
          message={ui("删除当前页面及全部子页面？此操作无法撤销。")}
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
  const unlocked = useNotesStore((s) => s.unlocked);
  useEffect(() => {
    let cancelled = false;
    setItems([]);
    api.listRecent(50).then((next) => { if (!cancelled) setItems(next); }).catch(() => {});
    return () => { cancelled = true; };
  }, [unlocked]);

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
  const unlocked = useNotesStore((s) => s.unlocked);
  const searchRequest = useRef(0);
  const [query, setQuery] = useState("");
  const [rootPageId, setRootPageId] = useState("");
  const [results, setResults] = useState<SearchResult[] | null>(null);
  const [elapsed, setElapsed] = useState<number | null>(null);

  const pages = useMemo(() => flattenPages(tree?.pages ?? []).filter(({ node }) => !node.is_encrypted || unlocked.includes(node.section_id)), [tree, unlocked]);
  useEffect(() => { setRootPageId(""); setResults(null); }, [spaceId]);
  useEffect(() => { ++searchRequest.current; setResults(null); }, [spaceId, unlocked]);

  async function runSearch(e: FormEvent) {
    e.preventDefault();
    if (!query.trim()) return;
    const request = ++searchRequest.current;
    const start = performance.now();
    const hits = await api.searchNotes(
      spaceId,
      query.trim(),
      100,
      rootPageId || null,
    );
    if (request !== searchRequest.current) return;
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
        <select value={rootPageId} onChange={(e) => setRootPageId(e.target.value)} aria-label={ui("搜索范围")} className="max-w-64 rounded border px-2 py-1.5 text-sm dark:bg-neutral-900">
          <option value="">{ui("全部页面")}</option>
          {pages.map(({ node, path }) => <option key={node.id} value={node.id}>{path}</option>)}
        </select>
        <button type="submit" className="rounded bg-blue-600 px-4 py-1.5 text-white">{ui("搜索")}</button>
      </form>

      {results && (
        <p className="mb-2 text-sm text-neutral-400">
          {results.length}{ui("条结果")}{elapsed !== null && ` · ${elapsed.toFixed(0)}ms`}
          {results.some((r) => r.from_unlocked) && ui("（含已解锁受保护页面）")}
        </p>
      )}
      <ul className="max-w-3xl space-y-2">
        {results?.map((r) => {
          return (
            <li key={r.page_id}>
              <button
                className="block w-full rounded border px-3 py-2 text-left hover:bg-neutral-50 dark:hover:bg-neutral-800"
                onClick={() => navigate(`/s/${spaceId}/page/${r.page_id}`)}
              >
                <div className="flex items-baseline gap-2">
                  <span className="font-medium">{r.title}</span>
                  <span className="text-xs text-neutral-400">
                    {r.path}
                    {r.from_unlocked && " 🔓"}
                  </span>
                </div>
                <div
                  className="mt-0.5 text-sm text-neutral-600 dark:text-neutral-300 [&_mark]:bg-yellow-200 dark:[&_mark]:bg-yellow-700"
                  dangerouslySetInnerHTML={{ __html: r.snippet }}
                />
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
