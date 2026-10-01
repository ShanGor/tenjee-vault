import type { MessageKey } from "../../i18n";

export const ACTIONS = [
  { id: "open-command-palette", label: "action.open-command-palette", keywords: "command palette search", shortcut: "Mod+K", scope: "app" },
  { id: "quick-note", label: "action.quick-note", keywords: "note create", shortcut: "Mod+Shift+N", scope: "system" },
  { id: "quick-task", label: "action.quick-task", keywords: "task create inbox", shortcut: "Mod+Shift+T", scope: "system" },
  { id: "go-notes", label: "action.go-notes", keywords: "notes navigation", shortcut: "", scope: "app" },
  { id: "go-tasks", label: "action.go-tasks", keywords: "tasks navigation", shortcut: "", scope: "app" },
  { id: "go-calendar", label: "action.go-calendar", keywords: "calendar navigation", shortcut: "", scope: "app" },
  { id: "go-settings", label: "action.go-settings", keywords: "settings preferences", shortcut: "", scope: "app" },
  { id: "lock-all", label: "action.lock-all", keywords: "lock security encrypted", shortcut: "Mod+Shift+L", scope: "system" },
] as const satisfies ReadonlyArray<{ id: string; label: MessageKey; keywords: string; shortcut: string; scope: "app" | "system" }>;

export type ActionId = typeof ACTIONS[number]["id"];

export function dispatchAppAction(id: ActionId) {
  window.dispatchEvent(new CustomEvent<ActionId>("app-action", { detail: id }));
}

/** A registry mistake would make a persisted shortcut ambiguous, so fail during startup. */
export function assertActionRegistry() {
  const ids = new Set(ACTIONS.map((action) => action.id));
  if (ids.size !== ACTIONS.length) throw new Error("Duplicate application action id");
}
