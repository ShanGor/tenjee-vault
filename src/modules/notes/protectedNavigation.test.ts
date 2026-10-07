import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn(async () => []));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
beforeEach(() => { vi.resetModules(); invoke.mockClear(); });

it("recognizes encoded page routes and ignores query changes, modules and malformed routes", async () => {
  const { pageFromHash } = await import("./protectedNavigation");
  expect(pageFromHash("#/notes/s/space/page/section%3Aprivate?view=read")).toEqual({ spaceId: "space", pageId: "section:private" });
  expect(pageFromHash("#/settings")).toBeNull();
  expect(pageFromHash("#/notes/s/space")).toBeNull();
  expect(pageFromHash("#/notes/s/space/page/%ZZ")).toBeNull();
});

it("captures and saves the outgoing editor before revoking keys or letting the next page load", async () => {
  const { syncProtectedNavigation, waitForProtectedNavigation } = await import("./protectedNavigation");
  const { registerPageSave } = await import("./pageSave");
  await syncProtectedNavigation("#/notes/s/space/page/a");
  invoke.mockClear();
  let finishSave!: () => void;
  const save = vi.fn(() => new Promise<void>(resolve => { finishSave = resolve; }));
  const unregister = registerPageSave(save);
  const navigation = syncProtectedNavigation("#/notes/s/space/page/b");
  unregister();
  expect(save).toHaveBeenCalledOnce();
  let readReady = false;
  const read = waitForProtectedNavigation().then(() => { readReady = true; });
  await Promise.resolve();
  expect(invoke).not.toHaveBeenCalled();
  expect(readReady).toBe(false);
  finishSave();
  await navigation;
  await read;
  expect(invoke).toHaveBeenCalledWith("protected_page_navigation_cmd", { spaceId: "space", pageId: "b" });
  await syncProtectedNavigation("#/notes/s/space/page/b?view=read");
  expect(invoke).toHaveBeenCalledOnce();
  await syncProtectedNavigation("#/tasks");
  expect(invoke).toHaveBeenLastCalledWith("protected_page_navigation_cmd", { spaceId: null, pageId: null });
});

it("revokes keys even if saving fails and resumes on the next navigation", async () => {
  const { syncProtectedNavigation } = await import("./protectedNavigation");
  const { registerPageSave } = await import("./pageSave");
  const unregister = registerPageSave(async () => { throw new Error("save failed"); });
  await expect(syncProtectedNavigation("#/notes/s/space/page/b")).rejects.toThrow("save failed");
  expect(invoke).toHaveBeenCalledOnce();
  unregister();
  await expect(syncProtectedNavigation("#/settings")).resolves.toBeUndefined();
});

it("invalidates an in-progress unlock even when navigation returns to the original page", async () => {
  const { syncProtectedNavigation, protectedNavigationRevision } = await import("./protectedNavigation");
  await syncProtectedNavigation("#/notes/s/space/page/a");
  const started = protectedNavigationRevision();
  await syncProtectedNavigation("#/notes/s/space/page/b");
  await syncProtectedNavigation("#/notes/s/space/page/a");
  expect(protectedNavigationRevision()).toBe(started + 2);
});
