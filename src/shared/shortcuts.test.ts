import { describe, expect, it } from "vitest";
import { actionForKeyboardEvent, normalizeShortcut, shortcutConflicts } from "./shortcuts";

describe("shortcut parsing", () => {
  it("normalizes supported combinations and detects duplicate action bindings", () => {
    expect(normalizeShortcut("Mod + Shift + K")).toBe("mod+shift+k");
    expect(normalizeShortcut("Ctrl+Unknown")).toBe("");
    expect(shortcutConflicts({ "quick-note": "Mod+K" })).toBe("open-command-palette:quick-note");
  });

  it("does not intercept composing input", () => {
    const event = { key: "k", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, isComposing: true, target: null } as KeyboardEvent;
    expect(actionForKeyboardEvent(event)).toBeNull();
  });
});
