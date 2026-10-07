export type FileEntry = { id: number; path: string[]; kind: "file" | "directory" | "excluded"; size: string; reason: string };
export type FilePreview = { batch: string; digest: string; files: number; directories: number; excluded: number; entries: number; totalBytes: string; destination: string; completed: number; phase: string };
export type FileProgress = { preview: FilePreview; current: string; transferredBytes: string; durableBytes: string; completed: number; bytesPerSecond: number; etaSeconds: number | null; confirmed: boolean };
export type FileIntent = { role: "send" | "receive"; batch: string | null; destination: string | null; acknowledgeExclusions: boolean };
export function formatFileBytes(value: string | bigint): string {
  const bytes = typeof value === "bigint" ? value : BigInt(value || "0");
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
  let unit = 0, divisor = 1n;
  while (bytes >= divisor * 1024n && unit < units.length - 1) { unit++; divisor *= 1024n; }
  return unit === 0 ? `${bytes} B` : `${bytes / divisor}.${(bytes % divisor) * 10n / divisor} ${units[unit]}`;
}
export function filePercent(bytes: string, total: string): number {
  const all = BigInt(total || "0");
  return all === 0n ? 0 : Math.min(100, Number(BigInt(bytes || "0") * 10000n / all) / 100);
}

export function fileOutcome(progress: FileProgress, phase: string): "active" | "complete" | "unconfirmed" | "partial" | "failed" {
  if (["discovering", "pairing", "negotiating", "approval", "exchanging", "verifying", "saving"].includes(phase)) return "active";
  if (phase === "finished" && progress.confirmed) return "complete";
  const accepted = progress.preview.files + progress.preview.directories;
  if (accepted > 0 && progress.completed === accepted) return "unconfirmed";
  return progress.completed > 0 || BigInt(progress.durableBytes) > 0n ? "partial" : "failed";
}
