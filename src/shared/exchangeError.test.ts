import { afterEach, expect, it } from "vitest";
import { setUILocale } from "../i18n/ui";
import { formatExchangeError } from "./exchangeError";

afterEach(() => setUILocale("en"));

it("identifies a peer failure and preserves its reason in both locales", () => {
  const detail = "Incompatible exchange schema; update both applications";
  const error = JSON.stringify({ code: "validation", params: [["detail", `Peer exchange failed: ${detail}`]] });
  setUILocale("en");
  expect(formatExchangeError(error)).toBe(`Peer device exchange failed: ${detail}`);
  setUILocale("zh-CN");
  expect(formatExchangeError(error)).toBe(`对方设备交换失败：${detail}`);
});

it("preserves disk and connection error details in both exchange UI locales", () => {
  for (const detail of ["Access is denied. (os error 5)", "Connection reset by peer (os error 104)"]) {
    const error = JSON.stringify({ code: "io", params: [["detail", detail]] });
    setUILocale("en");
    expect(formatExchangeError(error)).toBe(`Device exchange failed: ${detail}`);
    setUILocale("zh-CN");
    expect(formatExchangeError(new Error(error))).toBe(`设备交换失败：${detail}`);
  }
});

it("keeps scope validation and lifecycle guidance and formats other errors normally", () => {
  const detail = "Peer did not approve the same full-workspace scope";
  expect(formatExchangeError(JSON.stringify({ code: "validation", params: [["detail", detail]] }))).toBe(detail);
  expect(formatExchangeError("App suspended; fresh pairing is required")).toBe("App suspended; fresh pairing is required");
  expect(formatExchangeError(JSON.stringify({ code: "section_locked" }))).toBe("Page is locked");
});
