// @vitest-environment jsdom
import { act, Suspense } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter, Route, Routes, useNavigate } from "react-router-dom";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import PageView from "./PageView";
import { Editor } from "@tiptap/core";
import { api, type Page, type SpacePageNode } from "./api";
import { useNotesStore } from "./store";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn(async () => undefined) }));

let root: Root;
let host: HTMLDivElement;

function node(id: string): SpacePageNode {
  return { id, section_id: "domain", parent_page_id: null, title: id, sort_order: 0, updated_at: "", children: [], is_encrypted: true, protection_root_id: id };
}
function page(id: string, content: string): Page {
  return { id, section_id: "domain", parent_page_id: null, title: id, content, created_at: "", updated_at: "", sort_order: 0 };
}
function Navigation() {
  const navigate = useNavigate();
  return <>{["a", "b", "c", "d"].map(id => <button key={id} onClick={() => navigate(`/s/space/page/${id}`)}>go-{id}</button>)}</>;
}
async function flush() { await act(async () => { await Promise.resolve(); }); }
async function go(id: string) {
  const button = Array.from(host.querySelectorAll("button")).find(item => item.textContent === `go-${id}`)!;
  await act(async () => { button.click(); await Promise.resolve(); });
  await flush();
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  useNotesStore.setState({ currentSpaceId: "space", tree: { pages: ["a", "b", "c", "d"].map(node), unlocked_section_ids: ["domain"] }, unlocked: ["domain"] });
  vi.spyOn(api, "recordPageOpen").mockResolvedValue();
  vi.spyOn(api, "listPageTitles").mockResolvedValue([]);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals();
  useNotesStore.setState({ currentSpaceId: null, tree: null, unlocked: [] });
});

it("keeps the app mounted while repeatedly opening protected pages with legacy content", async () => {
  const documents: Record<string, string> = {
    a: JSON.stringify({ type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: "Alpha" }] }] }),
    b: JSON.stringify({ type: "doc", content: [{ type: "taskList", content: [{ type: "taskItem", attrs: { checked: true }, content: [{ type: "paragraph", content: [{ type: "text", text: "Legacy task" }] }] }] }] }),
    c: JSON.stringify({ type: "doc", content: [{ type: "codeBlock", attrs: { language: "javascript" }, content: [{ type: "text", text: "const answer = 42;" }] }] }),
    d: "Legacy plain text",
  };
  vi.spyOn(api, "getPage").mockImplementation(async (_space, id) => page(id, documents[id]));
  await act(async () => root.render(<MemoryRouter initialEntries={["/s/space/page/a"]}><Navigation /><Routes><Route path="/s/:spaceId/page/:pageId" element={<PageView />} /></Routes></MemoryRouter>));
  await flush();
  expect(host.textContent).toContain("Alpha");
  for (const id of ["b", "c", "d", "a", "c"]) {
    await go(id);
    expect(host.querySelector(".page-editor")).not.toBeNull();
  }
  expect(host.textContent).toContain("const answer = 42;");
});

it("keeps navigation available through repeated protected lock transitions", async () => {
  vi.spyOn(api, "getPage").mockImplementation(async (_space, id) => page(id, JSON.stringify({ type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: id }] }] })));
  await act(async () => root.render(<MemoryRouter initialEntries={["/s/space/page/a"]}><Navigation /><Routes><Route path="/s/:spaceId/page/:pageId" element={<PageView />} /></Routes></MemoryRouter>));
  await flush();
  await act(async () => { useNotesStore.setState({ unlocked: [] }); });
  await go("b");
  expect(host.textContent).toContain("This page is locked");
  await act(async () => { useNotesStore.setState({ unlocked: ["domain"] }); });
  await flush();
  expect(host.querySelector(".page-editor")).not.toBeNull();
});

// A concurrent route render can yield before passive effects mount the editor.
it("opens pages after a suspended route commit without using a destroyed editor", async () => {
  const mountEditor = vi.spyOn(Editor.prototype, "mount");
  let ready = false;
  let resume!: () => void;
  const pending = new Promise<void>(resolve => { resume = () => { ready = true; resolve(); }; });
  function DelayedSibling() { if (!ready) throw pending; return null; }
  vi.spyOn(api, "getPage").mockImplementation(async (_space, id) => page(id, JSON.stringify({ type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: `Content ${id}` }] }] })));
  await act(async () => root.render(<MemoryRouter initialEntries={["/s/space/page/a"]}><Navigation /><Suspense fallback={<p>Waiting</p>}><Routes><Route path="/s/:spaceId/page/:pageId" element={<PageView />} /></Routes><DelayedSibling /></Suspense></MemoryRouter>));
  expect(host.textContent).toContain("Waiting");
  expect(mountEditor).not.toHaveBeenCalled();
  // Let the editor's scheduled destruction run before the route commits.
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
  await act(async () => { resume(); await pending; });
  await flush();
  expect(host.textContent).toContain("Content a");
  for (const id of ["b", "c", "a", "d", "b", "a"]) {
    await go(id);
    expect(host.textContent).toContain(`Content ${id}`);
    expect(host.querySelector('[role="alert"]')).toBeNull();
  }
});
