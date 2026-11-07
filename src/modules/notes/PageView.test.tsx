// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter, Route, Routes, useNavigate, useParams } from "react-router-dom";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import PageView from "./PageView";

vi.mock("./EditorPage", () => ({
  default: function MockEditorPage() {
    const { pageId } = useParams();
    if (pageId === "broken") throw new Error("Editor fixture failed");
    return <article>Readable {pageId}</article>;
  },
}));

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

function Navigation() {
  const navigate = useNavigate();
  return <nav><button onClick={() => navigate("/s/space/page/good")}>Other page</button></nav>;
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove(); vi.unstubAllGlobals();
});

it("contains a page rendering failure and recovers on the next protected-page navigation", async () => {
  const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
  await act(async () => root.render(<MemoryRouter initialEntries={["/s/space/page/broken"]}><Navigation /><Routes><Route path="/s/:spaceId/page/:pageId" element={<PageView />} /></Routes></MemoryRouter>));
  expect(host.textContent).toContain("Other page");
  expect(host.textContent).toContain("This page could not be displayed");
  expect(host.textContent).toContain("Editor fixture failed");
  const other = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Other page")!;
  await act(async () => { other.click(); });
  expect(host.textContent).toContain("Readable good");
  expect(host.textContent).not.toContain("This page could not be displayed");
  consoleError.mockRestore();
});
