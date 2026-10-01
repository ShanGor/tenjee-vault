import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ACTIONS, type ActionId } from "./actions";
import { usePreferences } from "../i18n";

type GlobalHit = { kind: "note" | "task" | "calendar"; id: string; title: string; snippet: string; context: string; route: string; };
type Row = { kind: "action"; id: ActionId; title: string; context: string } | { kind: "result"; hit: GlobalHit; title: string; context: string };

export function CommandPanel({ open, onClose, onAction }: { open: boolean; onClose(): void; onAction(id: ActionId): void }) {
  const { t } = usePreferences();
  const input = useRef<HTMLInputElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<GlobalHit[]>([]);
  const [active, setActive] = useState(0);
  const requestId = useRef(0);

  const actions = ACTIONS.filter((action) => `${t(action.label)} ${action.keywords}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  const rows: Row[] = [...actions.map((action) => ({ kind: "action" as const, id: action.id as ActionId, title: t(action.label), context: t("command.actions") })), ...hits.map((hit) => ({ kind: "result" as const, hit, title: hit.title, context: hit.context }))];

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setQuery(""); setHits([]); setActive(0);
    queueMicrotask(() => input.current?.focus());
    return () => previousFocus.current?.focus();
  }, [open]);

  useEffect(() => {
    const currentRequest = ++requestId.current;
    if (!open || !query.trim()) { setHits([]); return; }
    const timer = window.setTimeout(() => {
      void invoke<GlobalHit[]>("global_search_cmd", { query, limit: 30 }).then((next) => {
        if (requestId.current === currentRequest) setHits(next);
      }).catch(() => {
        if (requestId.current === currentRequest) setHits([]);
      });
    }, 120);
    return () => window.clearTimeout(timer);
  }, [open, query]);

  useEffect(() => setActive((value) => Math.min(value, Math.max(0, rows.length - 1))), [query, hits.length, rows.length]);
  if (!open) return null;
  const run = (row: Row | undefined) => {
    if (!row) return;
    if (row.kind === "action") onAction(row.id);
    else window.location.hash = row.hit.route;
    onClose();
  };
  return <div className="fixed inset-0 z-[60] grid place-items-start bg-black/30 pt-[12vh]" role="presentation" onMouseDown={onClose}>
    <section className="w-[min(42rem,calc(100vw-2rem))] overflow-hidden rounded-xl bg-white shadow-2xl dark:bg-neutral-900" role="dialog" aria-modal="true" aria-label={t("command.label")} onMouseDown={(event) => event.stopPropagation()}>
      <input ref={input} className="w-full border-b bg-transparent px-4 py-3 text-lg outline-none" value={query} onChange={(event) => { setQuery(event.target.value); setActive(0); }} placeholder={t("command.placeholder")} onKeyDown={(event) => { if (event.key === "Escape") { event.preventDefault(); onClose(); } if (event.key === "ArrowDown") { event.preventDefault(); setActive((value) => Math.min(rows.length - 1, value + 1)); } if (event.key === "ArrowUp") { event.preventDefault(); setActive((value) => Math.max(0, value - 1)); } if (event.key === "Enter") { event.preventDefault(); run(rows[active]); } }} />
      <div className="max-h-[55vh] overflow-y-auto p-2">{rows.length === 0 && <p className="p-3 text-sm text-neutral-500">{t("command.empty")}</p>}{rows.map((row, index) => <div key={row.kind === "action" ? row.id : `${row.hit.kind}-${row.hit.id}`}>{startsGroup(row, rows[index - 1]) && <p className="px-3 pt-2 text-xs font-medium uppercase tracking-wide text-neutral-500">{row.kind === "action" ? t("command.actions") : resultGroup(row.hit.kind, t)}</p>}<button className={`block w-full rounded p-3 text-left ${index === active ? "bg-blue-100 dark:bg-blue-900" : "hover:bg-neutral-100 dark:hover:bg-neutral-800"}`} onMouseEnter={() => setActive(index)} onClick={() => run(row)}><div className="flex gap-2"><b className="flex-1">{row.title}</b><span className="text-xs text-neutral-500">{row.kind === "action" ? t("command.actions") : resultGroup(row.hit.kind, t)}</span></div>{row.kind === "result" && <div className="mt-1 text-sm text-neutral-500">{safeSnippet(row.hit.snippet)}</div>}<small className="block text-neutral-400">{row.context}</small></button></div>)}</div>
    </section>
  </div>;
}

function resultGroup(kind: GlobalHit["kind"], t: ReturnType<typeof usePreferences>["t"]) {
  return t((`result.${kind}`) as "result.note" | "result.task" | "result.calendar");
}

function startsGroup(row: Row, previous: Row | undefined) {
  if (!previous) return true;
  if (row.kind !== "result") return false;
  return previous.kind !== "result" || previous.hit.kind !== row.hit.kind;
}

// FTS returns only its own <mark> delimiters.  Convert them to React nodes instead of
// injecting the surrounding user-authored text as HTML.
function safeSnippet(snippet: string) {
  let marked = false;
  return snippet.split(/(<\/?mark>)/g).map((part, index) => {
    if (part === "<mark>") { marked = true; return null; }
    if (part === "</mark>") { marked = false; return null; }
    return marked ? <mark key={index}>{part}</mark> : <span key={index}>{part}</span>;
  });
}
