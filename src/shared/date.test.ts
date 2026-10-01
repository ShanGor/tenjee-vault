import { describe, expect, it } from "vitest";
import { monthGrid, startOfWeek, weekDates } from "./date";

describe("calendar date grids", () => {
  it("starts weeks on Monday across a year boundary", () => {
    expect(startOfWeek("2027-01-01")).toBe("2026-12-28");
    expect(weekDates("2027-01-01")).toEqual([
      "2026-12-28", "2026-12-29", "2026-12-30", "2026-12-31",
      "2027-01-01", "2027-01-02", "2027-01-03",
    ]);
  });

  it("builds complete month rows across adjacent months", () => {
    const february = monthGrid("2026-02-17");
    expect(february[0]).toBe("2026-01-26");
    expect(february[february.length - 1]).toBe("2026-03-01");
    expect(february).toHaveLength(35);
  });
});
