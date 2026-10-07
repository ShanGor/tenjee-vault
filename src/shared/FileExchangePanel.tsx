import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ui } from "../i18n/ui";
import { isAndroid } from "./nativeFiles";
import { formatFileBytes, filePercent, fileOutcome, type FileEntry, type FileIntent, type FilePreview, type FileProgress } from "./fileExchange";

type Selection = { handle: string; label: string };
type Destination = { handle: string; label: string; notice: string };
type Props = { active: boolean; busy: boolean; phase: string; progress: FileProgress | null; peerLabel: string; peerPlatform: string; onIntent: (intent: FileIntent) => void; action: (work: () => Promise<unknown>) => Promise<void> };
const button = "rounded border px-3 py-2 disabled:opacity-50";

export function FileExchangePanel({ active, busy, phase, progress, peerLabel, peerPlatform, onIntent, action }: Props) {
  const android = typeof navigator !== "undefined" && typeof window !== "undefined" && isAndroid();
  const [role, setRole] = useState<"send" | "receive">("send");
  const [selections, setSelections] = useState<Selection[]>([]);
  const [batch, setBatch] = useState<FilePreview | null>(null);
  const [destination, setDestination] = useState<Destination | null>(null);
  const [acknowledge, setAcknowledge] = useState(false);
  const [allowStaging, setAllowStaging] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [history, setHistory] = useState<FilePreview[]>([]);
  const visible = active ? progress?.preview ?? batch : batch ?? progress?.preview;
  const showProgress = progress && (active || !batch || batch.batch === progress.preview.batch);
  useEffect(() => { onIntent({ role, batch: batch?.batch ?? null, destination: destination?.handle ?? null, acknowledgeExclusions: acknowledge }); }, [role, batch, destination, acknowledge, onIntent]);
  useEffect(() => { if (!active) void invoke<FilePreview[]>("file_exchange_history_cmd").then(setHistory).catch(() => undefined); }, [active, phase]);
  useEffect(() => () => { void invoke("file_exchange_cancel_prepare_cmd").catch(() => undefined); }, []);
  async function add(folder: boolean) {
    const added = await invoke<Selection[]>("file_exchange_select_cmd", { folder });
    if (added.length) { setSelections(previous => [...previous, ...added]); setBatch(null); setAcknowledge(false); }
  }
  async function prepare() {
    setPreparing(true);
    try { setBatch(await invoke<FilePreview>("file_exchange_prepare_cmd", { handles: selections.map(s => s.handle), allowStaging })); setAcknowledge(false); }
    finally { setPreparing(false); }
  }
  return <section className="space-y-3 rounded-lg border p-4">
    <h2 className="font-medium">{ui("文件与文件夹")}</h2>
    {active && progress && <p>{ui("与 {p0} 共享此文件批次", { p0: peerLabel })} · {peerPlatform}</p>}
    <p className="text-sm text-neutral-600">{ui("选择要复制的文件或文件夹。接收方选择保存位置并确认后开始，文件夹结构会保留。")}</p>
    <div className="flex gap-2" role="group" aria-label={ui("传输方向")}>
      <button className={button} aria-pressed={role === "send"} disabled={active || busy} onClick={() => setRole("send")}>{ui("发送文件")}</button>
      <button className={button} aria-pressed={role === "receive"} disabled={active || busy} onClick={() => setRole("receive")}>{ui("接收文件")}</button>
    </div>
    {!active && role === "send" && <>
      <div className="flex flex-wrap gap-2"><button className={button} disabled={busy} onClick={() => void action(() => add(false))}>{ui("添加文件")}</button><button className={button} disabled={busy} onClick={() => void action(() => add(true))}>{ui("添加文件夹")}</button></div>
      {selections.map(selection => <div className="flex items-center justify-between gap-3" key={selection.handle}><span className="min-w-0 break-words">{selection.label}</span><button className={button} disabled={busy} aria-label={ui("移除选择")} onClick={() => { setSelections(s => s.filter(item => item.handle !== selection.handle)); setBatch(null); }}>{ui("移除")}</button></div>)}
      {android && <label className="flex gap-2 text-sm"><input type="checkbox" disabled={busy} checked={allowStaging} onChange={event => { setAllowStaging(event.target.checked); setBatch(null); }} />{ui("允许为无法直接读取的 Android 文档暂存副本（需要额外空间）")}</label>}
      <button className={button} disabled={busy || !selections.length} onClick={() => void action(prepare)}>{ui("准备并预览")}</button>
      {preparing && <div role="status">{ui("正在扫描所选内容")}<button className={`${button} ml-3`} onClick={() => void invoke("file_exchange_cancel_prepare_cmd")}>{ui("取消准备")}</button></div>}
    </>}
    {!active && role === "receive" && <>
      <button className={button} disabled={busy} onClick={() => void action(async () => { const selected = await invoke<Destination | null>("file_exchange_destination_cmd"); if (selected) setDestination(selected); })}>{ui("选择接收文件夹")}</button>
      <p className="break-words text-sm">{destination?.label ?? ui("请先选择保存位置")}</p>
      {destination?.notice && <p className="text-sm text-neutral-600">{ui("Android 文档提供方可能在保存时显示未完成文件，容量和持久性保证可能未知。请保持应用前台直到保存完成。")}</p>}
      <p className="text-sm text-neutral-600">{ui("收到的内容保存在新建批次文件夹中，不会覆盖已有文件。")}</p>
    </>}
    {visible && <>
      <p className="text-sm">{ui("{p0} 个文件，{p1} 个文件夹，共 {p2}", { p0: String(visible.files), p1: String(visible.directories), p2: formatFileBytes(visible.totalBytes) })}</p>
      {!!visible.excluded && <p className="text-sm">{ui("{p0} 项不支持或无法读取，已列为排除项。", { p0: String(visible.excluded) })}</p>}
      <FileTree key={visible.batch} batch={visible.batch} entries={visible.entries} action={action} />
      {role === "send" && !active && !!visible.excluded && <label className="flex gap-2 text-sm"><input type="checkbox" checked={acknowledge} onChange={event => setAcknowledge(event.target.checked)} />{ui("我已检查排除项，仅发送支持的内容")}</label>}
    </>}
    {showProgress && progress && <div className="space-y-2" role="status" aria-live="polite">
      <p className="break-words text-sm">{progress.current}</p>
      <progress className="w-full" max={100} value={filePercent(progress.transferredBytes, progress.preview.totalBytes)} aria-label={ui("文件传输进度")} />
      <p className="text-sm">{ui("已传输 {p0} / {p1}；已保存检查点 {p2}", { p0: formatFileBytes(progress.transferredBytes), p1: formatFileBytes(progress.preview.totalBytes), p2: formatFileBytes(progress.durableBytes) })}</p>
      <p className="text-sm">{ui("已完成 {p0} / {p1} 项", { p0: String(progress.completed), p1: String(progress.preview.files + progress.preview.directories) })}</p>
      {phase === "exchanging" && progress.bytesPerSecond > 0 && <p className="text-sm">{formatFileBytes(BigInt(Math.floor(progress.bytesPerSecond)))}/s{progress.etaSeconds !== null ? ` · ${ui("预计剩余 {p0} 秒", { p0: String(Math.ceil(progress.etaSeconds)) })}` : ""}</p>}
      {phase === "finished" && progress.confirmed && <p>{ui("接收方已确认所有接受的内容保存完成")}</p>}
      {fileOutcome(progress, phase) === "unconfirmed" && <p>{ui("尚未确认：文件回执已保存，但批次未得到最终确认。重新配对以核对结果，不会重复已验证的文件。")}</p>}
      {fileOutcome(progress, phase) === "partial" && <p>{ui("部分完成：已保存的文件和检查点会保留。重新配对后可继续剩余内容。")}</p>}
      {fileOutcome(progress, phase) === "failed" && <p>{ui("本次传输失败或已取消，尚未保存接受的内容。请检查错误并重新配对。")}</p>}
      {progress.preview.destination && <p className="break-words text-sm">{progress.preview.destination}</p>}
    </div>}
    {phase === "approval" && progress && <div className="flex gap-2"><button className={button} disabled={busy} onClick={() => void action(() => invoke("exchange_approve_cmd"))}>{ui("确认此文件批次")}</button><button className={button} disabled={busy} onClick={() => void action(() => invoke("exchange_stop_cmd"))}>{ui("拒绝")}</button></div>}
    {!active && history.length > 0 && <details><summary>{ui("文件传输历史与继续传输")}</summary><div className="mt-3 space-y-3">{history.map(item => <article className="space-y-2 rounded border p-3" key={item.batch}>
      <p className="break-words text-sm">{item.batch.slice(0, 8)} · {formatFileBytes(item.totalBytes)} · {ui("已完成 {p0} / {p1} 项", { p0: String(item.completed), p1: String(item.files + item.directories) })}</p>
      <div className="flex flex-wrap gap-2">
        {!item.destination && <button className={button} disabled={busy} onClick={() => { setRole("send"); setBatch(item); setAcknowledge(false); }}>{ui("继续发送此批次")}</button>}
        {!!item.destination && <button className={button} disabled={busy} onClick={() => void action(() => invoke("file_exchange_open_destination_cmd", { batch: item.batch }))}>{ui("打开接收文件夹")}</button>}
        <button className={button} disabled={busy} onClick={() => { if (window.confirm(ui("丢弃此批次的未完成文件和恢复记录？原文件和已完成的文件会保留。"))) void action(async () => { await invoke("file_exchange_discard_cmd", { batch: item.batch }); setHistory(await invoke<FilePreview[]>("file_exchange_history_cmd")); if (batch?.batch === item.batch) setBatch(null); }); }}>{ui("丢弃未完成文件")}</button>
      </div>
      {!!item.destination && item.phase !== "complete" && <p className="text-sm">{ui("接收方重新选择原保存位置并开启接收，发送方选择原批次并重新配对。")}</p>}
    </article>)}</div></details>}
  </section>;
}

function FileTree({ batch, entries, action }: { batch: string; entries: number; action: Props["action"] }) {
  const [start, setStart] = useState(0);
  const [rows, setRows] = useState<FileEntry[]>([]);
  useEffect(() => { let alive = true; void action(async () => { const page = await invoke<FileEntry[]>("file_exchange_preview_cmd", { batch, start }); if (alive) setRows(page); }); return () => { alive = false; }; }, [batch, start]);
  return <details><summary>{ui("查看文件与排除项")}</summary><ul className="my-2 max-h-64 space-y-1 overflow-y-auto text-sm">{rows.map(entry => <li key={entry.id} className="break-words">{entry.path.join("/")}{entry.kind === "directory" ? "/" : entry.kind === "file" ? ` · ${formatFileBytes(entry.size)}` : ` · ${entry.reason}`}</li>)}</ul><div className="flex gap-2"><button className={button} disabled={start === 0} onClick={() => setStart(Math.max(0, start - 100))}>{ui("上一页")}</button><button className={button} disabled={start + 100 >= entries} onClick={() => setStart(start + 100)}>{ui("下一页")}</button></div></details>;
}
