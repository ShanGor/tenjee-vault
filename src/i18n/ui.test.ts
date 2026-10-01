import { afterEach, expect, it } from "vitest";
import { ui, setUILocale, uiEnglish, uiError, commandErrorCode } from "./ui";
afterEach(() => setUILocale("en"));
it("switches every UI catalog entry and preserves user-provided message parameters", () => {
  for(const key of Object.keys(uiEnglish) as (keyof typeof uiEnglish)[]) {
    setUILocale("en"); expect(ui(key)).toBe(uiEnglish[key]);
    setUILocale("zh-CN"); expect(ui(key)).toBe(key);
  }
  setUILocale("en");
  expect(ui("删除附件 {p0}？",{p0:"用户文件.txt"})).toBe("Delete attachment 用户文件.txt?");
});
it("uses stable error codes for locked pages and localizes template password-removal guidance", () => {
  const locked=JSON.stringify({code:"section_locked",params:[["section","id"]]});
  expect(commandErrorCode(locked)).toBe("section_locked");
  expect(uiError(locked)).toBe("Section is locked");
  const blocked=JSON.stringify({code:"validation",params:[["detail","请先导出并删除或直接删除此分区的加密模板，再移除密码"]]});
  expect(uiError(blocked)).toContain("before removing its password");
  setUILocale("zh-CN"); expect(uiError(locked)).toBe("分区已锁定");
});
