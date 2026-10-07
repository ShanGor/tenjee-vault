import { describe, expect, it } from "vitest";
import { filePercent, formatFileBytes } from "./fileExchange";
describe("file transfer display", () => {
  it("keeps 64-bit byte values exact without Number conversion", () => {
    expect(formatFileBytes("8589934592")).toBe("8.0 GiB");
    expect(formatFileBytes("18446744073709551615")).toBe("15.9 EiB");
    expect(filePercent("4503599627370496", "9007199254740992")).toBe(50);
  });
  it("bounds progress and handles empty batches", () => {
    expect(filePercent("0", "0")).toBe(0);
    expect(filePercent("12", "10")).toBe(100);
    expect(formatFileBytes("0")).toBe("0 B");
  });
});
