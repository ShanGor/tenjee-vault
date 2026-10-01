import { ACTIONS, type ActionId } from "./actions";

export type ShortcutMap = Partial<Record<ActionId, string>>;

export function defaultShortcuts(): ShortcutMap {
  return Object.fromEntries(ACTIONS.filter((action) => action.shortcut).map((action) => [action.id, action.shortcut]));
}

export function normalizeShortcut(value: string): string {
  const parts = value.trim().split("+").map((part) => part.trim().toLowerCase()).filter(Boolean);
  const key = parts.pop();
  if (!key || parts.length > 3 || !/^[a-z0-9]$|^arrow(?:up|down|left|right)$|^escape$|^enter$/.test(key)) return "";
  const modifiers = new Set(parts.map((part) => part === "cmdorctrl" ? "mod" : part));
  if ([...modifiers].some((part) => !["mod", "ctrl", "alt", "shift"].includes(part))) return "";
  return [...["mod", "ctrl", "alt", "shift"].filter((part) => modifiers.has(part)), key].join("+");
}

export function shortcutConflicts(shortcuts: ShortcutMap): string | null {
  const seen = new Map<string, ActionId>();
  for (const action of ACTIONS) {
    const value = normalizeShortcut(shortcuts[action.id] ?? action.shortcut);
    if (!value) continue;
    const conflict = seen.get(value);
    if (conflict) return [conflict, action.id].join(":");
    seen.set(value, action.id);
  }
  return null;
}

function editableTarget(target: EventTarget | null) {
  return typeof HTMLElement !== "undefined" && target instanceof HTMLElement && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));
}

export function actionForKeyboardEvent(event: KeyboardEvent, shortcuts: ShortcutMap = defaultShortcuts()): ActionId | null {
  if (event.isComposing || editableTarget(event.target)) return null;
  const key = event.key.toLowerCase();
  const modifiers = [event.metaKey || event.ctrlKey ? "mod" : "", event.altKey ? "alt" : "", event.shiftKey ? "shift" : ""].filter(Boolean);
  const pressed = [...modifiers, key].join("+");
  for (const action of ACTIONS) {
    if (normalizeShortcut(shortcuts[action.id] ?? action.shortcut) === pressed) return action.id;
  }
  return null;
}
