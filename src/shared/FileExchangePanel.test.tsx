import { afterEach, describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { setUILocale } from "../i18n/ui";
import { FileExchangePanel } from "./FileExchangePanel";
import { fileOutcome, type FileProgress } from "./fileExchange";
const progress: FileProgress = {
  preview: { batch: "batch", digest: "hash", files: 1, directories: 1, excluded: 0, entries: 2, totalBytes: "8589934592", destination: "Receive folder", completed: 0, phase: "transferring" },
  current: "folder/文件.bin", transferredBytes: "8589934592", durableBytes: "8589934592", completed: 1, bytesPerSecond: 0, etaSeconds: null, confirmed: false,
};
afterEach(() => setUILocale("en"));
describe("file exchange result and approval display", () => {
  it("keeps saving active at 100 percent and distinguishes partial, unconfirmed and complete", () => {
    expect(fileOutcome(progress, "saving")).toBe("active");
    expect(fileOutcome(progress, "failed")).toBe("partial");
    expect(fileOutcome({ ...progress, completed: 2 }, "failed")).toBe("unconfirmed");
    expect(fileOutcome({ ...progress, completed: 2, confirmed: true }, "finished")).toBe("complete");
    expect(fileOutcome({ ...progress, completed: 0, durableBytes: "0" }, "stopped")).toBe("failed");
  });
  it("renders the authenticated peer, exact large totals and approval in both locales", () => {
    for (const locale of ["en", "zh-CN"]) {
      setUILocale(locale);
      const html = renderToStaticMarkup(<FileExchangePanel active busy={false} phase="approval" progress={progress} peerLabel="Paired laptop" peerPlatform="windows" onIntent={() => undefined} action={async () => undefined} />);
      expect(html).toContain("Paired laptop"); expect(html).toContain("windows"); expect(html).toContain("8.0 GiB");
      expect(html).toContain(locale === "en" ? "Approve this file batch" : "确认此文件批次");
      expect(html).not.toContain(locale === "en" ? "recipient confirmed" : "接收方已确认所有");
      expect(html).not.toContain("NaN");
    }
  });
});
