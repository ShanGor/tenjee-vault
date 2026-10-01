import { ui } from "../../i18n/ui";
// 应用路由：三栏导航深链 + 回收站/最近/搜索视图。

import { useEffect } from "react";
import { HashRouter, Navigate, Route, Routes, useNavigate, useParams } from "react-router-dom";
import AppShell from "./AppShell";
import NotebookView from "./NotebookView";
import { RecentView, SearchView, TrashView } from "./views";
import { api } from "./api";
import { useNotesStore } from "./store";

/** 简写深链 /s/:spaceId/page/:pageId → 解析出笔记本与分区后跳到完整路径。 */
function PageRedirect() {
  const { spaceId = "", pageId = "" } = useParams();
  const navigate = useNavigate();
  const tree = useNotesStore((s) => s.tree);

  useEffect(() => {
    (async () => {
      try {
        const page = await api.getPage(spaceId, pageId);
        const find = (notebookId: string) =>
          navigate(`/s/${spaceId}/nb/${notebookId}/sec/${page.section_id}/page/${pageId}`, { replace: true });
        for (const nb of tree?.notebooks ?? []) {
          if (nb.sections.some((s) => s.id === page.section_id)) return find(nb.id);
          const stack = [...nb.groups];
          while (stack.length) {
            const g = stack.pop()!;
            if (g.sections.some((s) => s.id === page.section_id)) return find(nb.id);
            stack.push(...g.children);
          }
        }
      } catch {
        navigate(`/s/${spaceId}`, { replace: true });
      }
    })();
  }, [spaceId, pageId, tree, navigate]);

  return <div className="p-8 text-neutral-400">{ui("跳转中…")}</div>;
}

export default function App() {
  return (
    <HashRouter basename="/notes">
      <Routes>
        <Route element={<AppShell />}>
          <Route path="/" element={<div className="p-8 text-neutral-400">{ui("选择一个空间开始")}</div>} />
          <Route path="/s/:spaceId" element={<NotebookView />} />
          <Route path="/s/:spaceId/nb/:notebookId" element={<NotebookView />} />
          <Route path="/s/:spaceId/nb/:notebookId/sec/:sectionId" element={<NotebookView />} />
          <Route path="/s/:spaceId/nb/:notebookId/sec/:sectionId/page/:pageId" element={<NotebookView />} />
          <Route path="/s/:spaceId/page/:pageId" element={<PageRedirect />} />
          <Route path="/trash" element={<TrashView />} />
          <Route path="/recent" element={<RecentView />} />
          <Route path="/search" element={<SearchView />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
