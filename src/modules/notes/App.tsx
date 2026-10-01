import { HashRouter, Navigate, Route, Routes, useParams } from "react-router-dom";
import AppShell from "./AppShell";
import PageView from "./PageView";
import { RecentView, SearchView, TrashView } from "./views";

function LegacyPageRedirect() {
  const { spaceId = "", notebookId, sectionId, pageId } = useParams();
  const id = pageId || (sectionId ? `section:${sectionId}` : `notebook:${notebookId}`);
  return <Navigate to={`/s/${spaceId}/page/${encodeURIComponent(id)}`} replace />;
}

export default function App() {
  return <HashRouter basename="/notes"><Routes><Route element={<AppShell />}>
    <Route path="/" element={<Navigate to="/recent" replace />} />
    <Route path="/s/:spaceId" element={<PageView />} />
    <Route path="/s/:spaceId/page/:pageId" element={<PageView />} />
    <Route path="/s/:spaceId/nb/:notebookId" element={<LegacyPageRedirect />} />
    <Route path="/s/:spaceId/nb/:notebookId/sec/:sectionId" element={<LegacyPageRedirect />} />
    <Route path="/s/:spaceId/nb/:notebookId/sec/:sectionId/page/:pageId" element={<LegacyPageRedirect />} />
    <Route path="/trash" element={<TrashView />} />
    <Route path="/recent" element={<RecentView />} />
    <Route path="/search" element={<SearchView />} />
    <Route path="*" element={<Navigate to="/" replace />} />
  </Route></Routes></HashRouter>;
}
