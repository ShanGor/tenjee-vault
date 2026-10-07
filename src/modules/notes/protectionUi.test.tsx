import { afterEach, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { MemoryRouter } from "react-router-dom";
import { setUILocale } from "../../i18n/ui";
import { UnlockDialog } from "./dialogs";
import PageSidebar from "./PageSidebar";
import { useNotesStore } from "./store";
import { type SpacePageNode } from "./api";

vi.mock("./store", async importOriginal => {
  const actual = await importOriginal<typeof import("./store")>();
  const snapshot = (selector?: (state: ReturnType<typeof actual.useNotesStore.getState>) => unknown) => {
    const state = actual.useNotesStore.getState();
    return selector ? selector(state) : state;
  };
  return { ...actual, useNotesStore: Object.assign(snapshot, actual.useNotesStore) };
});
vi.mock("../../shared/viewState", () => ({ readViewState: () => ({ space: ["root"] }), writeViewState: vi.fn() }));
afterEach(() => { setUILocale("en"); vi.restoreAllMocks(); useNotesStore.setState({ tree: null, currentSpaceId: null, unlocked: [], settings: null }); });

it("shows identifiable titles and expanded children for locked protected trees", () => {
  const child: SpacePageNode = { id: "child", section_id: "domain", parent_page_id: "root", title: "Budget", sort_order: 0, updated_at: "", children: [], is_encrypted: true, protection_root_id: "root" };
  const root = { ...child, id: "root", parent_page_id: null, title: "Private project", children: [child] };
  useNotesStore.setState({ currentSpaceId: "space", tree: { pages: [root], unlocked_section_ids: [] }, unlocked: [] });
  const html = renderToStaticMarkup(<MemoryRouter><PageSidebar /></MemoryRouter>);
  expect(html).toContain("Private project");
  expect(html).toContain("Budget");
  expect(html).toContain('data-page-id="child"');
});

it("offers both durations with switching pages selected by default in both languages", () => {
  for (const locale of ["en", "zh-CN"]) {
    setUILocale(locale);
    const html = renderToStaticMarkup(<UnlockDialog spaceId="space" sectionId="domain" pageId="page" onClose={() => undefined} />);
    expect(html).toMatch(/<input(?=[^>]*value="page")(?=[^>]*checked="")[^>]*>/);
    expect(html).not.toMatch(/<input(?=[^>]*value="app")(?=[^>]*checked="")[^>]*>/);
    expect(html).toContain(locale === "en" ? "Until switching to another page (default)" : "直到切换到其他页面（默认）");
    expect(html).toContain(locale === "en" ? "Until the app closes" : "直到应用关闭");
  }
});
