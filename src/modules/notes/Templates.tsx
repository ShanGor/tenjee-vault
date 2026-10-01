import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { usePreferences } from "../../i18n";
import type { PageNode } from "./api";

const paragraph = (text: string) => ({ type: "paragraph", content: text ? [{ type: "text", text }] : [] });
export const builtinTemplates = [
  { id: "blank", label: "templates.blank" as const, document: { type: "doc", content: [paragraph("")] } },
  { id: "meeting", label: "templates.meeting" as const, document: { type: "doc", content: [{ type: "heading", attrs: { level: 2 }, content: [] }, paragraph(""), { type: "taskList", content: [{ type: "taskItem", attrs: { checked: false }, content: [paragraph("")] }] }] } },
  { id: "journal", label: "templates.journal" as const, document: { type: "doc", content: [{ type: "heading", attrs: { level: 2 }, content: [] }, paragraph(""), paragraph("")] } },
];
type Template = { id: string; name: string; scope: "global" | "section" };

export function SaveTemplateButton({ spaceId, pageId, encrypted, beforeSave }: { spaceId: string; pageId: string; encrypted: boolean; beforeSave(): Promise<void> }) {
  const { t, formatError } = usePreferences();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [global, setGlobal] = useState(!encrypted);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function save() {
    const confirmPlaintext = encrypted && global;
    if (confirmPlaintext && !window.confirm(t("templates.plaintext-warning"))) return;
    setBusy(true); setError("");
    try {
      await beforeSave();
      await invoke("save_page_template", { spaceId, pageId, name, global, confirmPlaintext });
      setOpen(false); setName("");
    } catch (reason) { setError(formatError(reason)); }
    finally { setBusy(false); }
  }
  return <><button className="shrink-0 rounded border px-2 py-1 text-sm" onClick={() => { setGlobal(!encrypted); setOpen(true); }}>{t("templates.save")}</button>
    {open && <div className="fixed inset-0 z-50 grid place-items-center bg-black/30"><section role="dialog" aria-modal="true" aria-label={t("templates.save")} className="w-96 max-w-full rounded bg-white p-5 dark:bg-neutral-900" onKeyDown={(event) => { if (event.key === "Escape" && !busy) setOpen(false); }}>
      <h2>{t("templates.save")}</h2><input autoFocus className="my-3 w-full rounded border p-2" aria-label={t("templates.name")} value={name} onChange={(event) => setName(event.target.value)} />
      {encrypted && <label><input type="checkbox" checked={global} onChange={(event) => setGlobal(event.target.checked)} /> {t("templates.global-plaintext")}</label>}
      {encrypted && !global && <p>{t("templates.section-only")}</p>}
      {error && <p role="alert">{error}</p>}
      <div className="mt-4 flex gap-3"><button disabled={busy || !name.trim()} onClick={() => void save()}>{t("common.save")}</button><button disabled={busy} onClick={() => setOpen(false)}>{t("common.cancel")}</button></div>
    </section></div>}
  </>;
}

export function TemplateDialog({ spaceId, sectionId, parentPageId, onCreated, onClose }: { spaceId: string; sectionId: string; parentPageId: string | null; onCreated(page: PageNode): Promise<void>; onClose(): void }) {
  const { t, formatError } = usePreferences();
  const [templates, setTemplates] = useState<Template[]>([]);
  const [selected, setSelected] = useState("builtin:blank");
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const refresh = async () => setTemplates(await invoke<Template[]>("list_templates", { spaceId, sectionId }));
  useEffect(() => {
    const pending = listen<{ section_id: string }>("section-locked", (event) => { if (event.payload.section_id === sectionId) { setTemplates([]); onClose(); } });
    return () => { void pending.then((dispose) => dispose()); };
  }, [sectionId, onClose]);
  useEffect(() => { void refresh().catch((reason) => setError(formatError(reason))); }, [spaceId, sectionId]);
  async function create() {
    setBusy(true); setError("");
    try {
      const [scope, id] = selected.split(":");
      const builtin = scope === "builtin" ? builtinTemplates.find((item) => item.id === id)?.document : null;
      const page = await invoke<PageNode>("create_page_from_template", { spaceId, sectionId, parentPageId, title: title.trim() || t("templates.untitled"), id, scope, builtin });
      await onCreated(page);
    } catch (reason) { setError(formatError(reason)); }
    finally { setBusy(false); }
  }
  async function edit(template: Template, action: "rename" | "delete" | "export") {
    const { id, scope } = template;
    const name = action === "delete" ? null : window.prompt(t("templates.name"), template.name);
    if (action !== "delete" && !name?.trim()) return;
    if (action === "delete" && !window.confirm(t("templates.delete-confirm"))) return;
    if (action === "export" && !window.confirm(t("templates.plaintext-warning"))) return;
    setBusy(true); setError("");
    try {
      if (action === "export") await invoke("export_template_global", { spaceId, sectionId, id, name, confirmed: true });
      else await invoke("edit_template", { spaceId, sectionId, id, scope, name });
      if (action === "delete" && selected === `${scope}:${id}`) setSelected("builtin:blank");
      await refresh();
    } catch (reason) { setError(formatError(reason)); }
    finally { setBusy(false); }
  }
  return <div className="fixed inset-0 z-50 grid place-items-center bg-black/30"><section role="dialog" aria-modal="true" aria-label={t("templates.create")} className="max-h-[90vh] w-[32rem] max-w-full overflow-auto rounded bg-white p-5 dark:bg-neutral-900" onKeyDown={(event) => { if (event.key === "Escape" && !busy) onClose(); }}>
    <h2 className="text-lg font-semibold">{t("templates.create")}</h2>
    <label className="mt-3 block">{t("quick.title")}<input autoFocus className="block w-full rounded border p-2" value={title} onChange={(event) => setTitle(event.target.value)} /></label>
    <label className="my-3 block">{t("templates.choose")}<select disabled={busy} className="block w-full rounded border p-2" value={selected} onChange={(event) => setSelected(event.target.value)}>
      {builtinTemplates.map((item) => <option key={item.id} value={`builtin:${item.id}`}>{t(item.label)}</option>)}
      {templates.map((item) => <option key={`${item.scope}:${item.id}`} value={`${item.scope}:${item.id}`}>{item.name} — {t(item.scope === "section" ? "templates.section" : "templates.global")}</option>)}
    </select></label>
    <details><summary>{t("templates.manage")}</summary><p className="my-2 text-sm">{t("templates.password-rule")}</p>
      {templates.map((item) => <div className="flex flex-wrap gap-3 border-b py-2" key={item.id}><span className="flex-1">{item.name}</span><button disabled={busy} onClick={() => void edit(item,"rename")}>{t("tags.rename")}</button><button disabled={busy} onClick={() => void edit(item,"delete")}>{t("templates.delete")}</button>{item.scope === "section" && <button disabled={busy} onClick={() => void edit(item,"export")}>{t("templates.export")}</button>}</div>)}
    </details>
    {error && <p role="alert">{error}</p>}
    <div className="mt-4 flex gap-3"><button disabled={busy} onClick={() => void create()}>{t("templates.create")}</button><button disabled={busy} onClick={onClose}>{t("common.cancel")}</button></div>
  </section></div>;
}
