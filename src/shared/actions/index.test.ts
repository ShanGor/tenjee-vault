import { describe, expect, it } from "vitest";
import { ACTIONS, assertActionRegistry } from ".";

describe("application action registry", () => {
  it("has a unique stable id for every action", () => {
    expect(() => assertActionRegistry()).not.toThrow();
    expect(new Set(ACTIONS.map((action) => action.id)).size).toBe(ACTIONS.length);
  });
});
