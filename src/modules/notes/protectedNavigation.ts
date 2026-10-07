import { invoke } from "@tauri-apps/api/core";
import { flushPageSave } from "./pageSave";

export type UnlockDuration = "page" | "app";

export function pageFromHash(hash: string): { spaceId: string; pageId: string } | null {
  const match = /^#\/notes\/s\/([^/]+)\/page\/([^/?#]+)(?:[?#].*)?$/.exec(hash);
  if (!match) return null;
  try { return { spaceId: decodeURIComponent(match[1]), pageId: decodeURIComponent(match[2]) }; }
  catch { return null; }
}

let currentPage: string | null | undefined;
let revision = 0;
export function protectedNavigationRevision(): number { return revision; }
let navigation: Promise<unknown> = Promise.resolve();

// All page reads wait for the outgoing save and key revocation. This also covers
// back/forward, module changes, and navigation between children of one protected tree.
export function syncProtectedNavigation(hash: string): Promise<unknown> {
  const page = pageFromHash(hash);
  const key = page ? JSON.stringify([page.spaceId, page.pageId]) : null;
  if (key === currentPage) return navigation;
  currentPage = key;
  ++revision;
  // Capture the outgoing editor's save callback before React unmounts it.
  const saved = flushPageSave().then(() => ({ error: undefined as unknown }), error => ({ error }));
  navigation = navigation.catch(() => undefined).then(async () => {
    const result = await saved;
    await invoke<string[]>("protected_page_navigation_cmd", {
      spaceId: page?.spaceId ?? null, pageId: page?.pageId ?? null,
    });
    if (result.error !== undefined) throw result.error;
  });
  return navigation;
}

export async function waitForProtectedNavigation(): Promise<void> {
  // A quick second navigation can arrive while the first is still saving.
  let pending: Promise<unknown>;
  do { pending = navigation; await pending; } while (pending !== navigation);
}
