import { describe, expect, it } from "vitest";
import { actionForKeyboardEvent, normalizeShortcut, shortcutConflicts } from "./shortcuts";

describe("shortcut parsing", () => {
  it("normalizes supported combinations and detects duplicate action bindings", () => {
    expect(normalizeShortcut("Mod + Shift + K")).toBe("mod+shift+k");
    expect(normalizeShortcut("Ctrl+Unknown")).toBe("");
    expect(shortcutConflicts({ "quick-note": "Mod+K" })).toBe("open-command-palette:quick-note");
  });

  it("opens quick tasks with Ctrl/Cmd+T and releases the old default", () => {
    for (const mac of [false, true]) {
      const event = { key: "t", ctrlKey: !mac, metaKey: mac, altKey: false, shiftKey: false, isComposing: false, target: null } as KeyboardEvent;
      expect(actionForKeyboardEvent(event)).toBe("quick-task");
      expect(actionForKeyboardEvent({ ...event, shiftKey: true } as KeyboardEvent)).toBeNull();
    }
  });

  it("does not intercept composing input", () => {
    const event = { key: "k", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, isComposing: true, target: null } as KeyboardEvent;
    expect(actionForKeyboardEvent(event)).toBeNull();
  });
});
