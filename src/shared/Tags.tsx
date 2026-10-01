import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { usePreferences } from "../i18n";

type Tag = { id: string; name: string; color: string | null };
type Kind = "note" | "task" | "calendar";
type Results = {
  tag: Tag | null;
  items: { kind: Kind; id: string; title: string; route: string; status: string | null; archived: boolean }[];
  counts: Record<Kind, number>;
  unavailable_spaces: string[];
};
type Target = { kind: "page" | "task" | "event"; id: string; spaceId?: string };
const listTags = () => invoke<Tag[]>("list_tags");

export function TagSelector({ kind, id, spaceId }: Target) {
  const { t, formatError } = usePreferences();
  const [tags, setTags] = useState<Tag[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(true);
  const args = { [`${kind}Id`]: id, ...(spaceId ? { spaceId } : {}) };
  useEffect(() => {
    let active = true;
    setSelected([]); setBusy(true); setError("");
    void Promise.all([listTags(), invoke<string[]>(`get_${kind}_tags`, args)])
      .then(([dictionary, ids]) => { if (active) { setTags(dictionary); setSelected(ids); } })
      .catch((reason) => { if (active) setError(formatError(reason)); })
      .finally(() => { if (active) setBusy(false); });
    return () => { active = false; };
  }, [kind, id, spaceId, formatError]);
  async function change(ids: string[]) {
    setBusy(true); setError("");
    try { await invoke(`set_${kind}_tags`, { ...args, tagIds: ids }); setSelected(ids); }
    catch (reason) { setError(formatError(reason)); }
    finally { setBusy(false); }
  }
  async function create() {
    setBusy(true); setError("");
    try {
      const tag = await invoke<Tag>("create_tag", { name: name.trim(), color: null });
      setTags(await listTags()); setName("");
      await change([...selected, tag.id]);
    } catch (reason) { setError(formatError(reason)); }
    finally { setBusy(false); }
  }
  const missing = selected.filter((id) => !tags.some((tag) => tag.id === id));
  return <fieldset className="m-2 rounded border p-2" disabled={busy}>
    <legend>{t("tags.title")}</legend>
    <div className="flex flex-wrap gap-3">{tags.map((tag) => <label key={tag.id} className="flex items-center gap-1 text-sm">
      <input type="checkbox" checked={selected.includes(tag.id)} onChange={(event) => void change(event.target.checked ? [...selected, tag.id] : selected.filter((id) => id !== tag.id))} />
      {tag.name}</label>)}
      {missing.map((id) => <button key={id} onClick={() => void change(selected.filter((value) => value !== id))}>{t("tags.remove-missing")}</button>)}
    </div>
    <div className="mt-2 flex gap-2"><input className="min-w-0 rounded border p-1" aria-label={t("tags.new")} placeholder={t("tags.new")} value={name} onChange={(event) => setName(event.target.value)} />
      <button disabled={!name.trim()} onClick={() => void create()}>{t("tags.create")}</button>
      <a href="#/tags">{t("tags.browse")}</a></div>
    {error && <p role="alert">{error}</p>}
  </fieldset>;
}

export function TagsPage() {
  const { t, formatError } = usePreferences();
  const [tags, setTags] = useState<Tag[]>([]);
  const [tagId, setTagId] = useState("");
  const [kind, setKind] = useState<Kind | "">("");
  const [archived, setArchived] = useState(false);
  const [results, setResults] = useState<Results | null>(null);
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true;
    void listTags().then((next) => { if (active) { setTags(next); setTagId((id) => next.some((tag) => tag.id === id) ? id : next[0]?.id ?? ""); } }).catch((reason) => { if (active) setError(formatError(reason)); });
    return () => { active = false; };
  }, [revision, formatError]);
  useEffect(() => {
    let active = true;
    let generation = 0;
    const reload = () => {
      const request = ++generation;
      setResults(null);
      if (!tagId) return;
      void invoke<Results>("query_tag", { tagId, kind: kind || null, includeArchived: archived })
        .then((next) => { if (active && request === generation) setResults(next); })
        .catch((reason) => { if (active && request === generation) setError(formatError(reason)); });
    };
    reload();
    const locked = listen("section-locked", reload);
    window.addEventListener("focus", reload);
    return () => { active = false; ++generation; window.removeEventListener("focus", reload); void locked.then((dispose) => dispose()); };
  }, [tagId, kind, archived, revision, formatError]);
  async function edit(remove: boolean) {
    const tag = tags.find((tag) => tag.id === tagId);
    if (!tag) return;
    if (remove && !window.confirm(t("tags.confirm-delete"))) return;
    const name = remove ? null : window.prompt(t("tags.rename"), tag.name);
    if (!remove && !name?.trim()) return;
    try { await invoke(remove ? "delete_tag" : "update_tag", { id: tagId, name, color: tag.color }); setResults(null); setRevision((value) => value + 1); }
    catch (reason) { setError(formatError(reason)); }
  }
  return <main className="h-full overflow-auto p-6"><h1 className="text-2xl font-semibold">{t("tags.title")}</h1>
    <div className="my-4 flex flex-wrap gap-3">
      <select aria-label={t("tags.title")} value={tagId} onChange={(event) => setTagId(event.target.value)}>{tags.map((tag) => <option key={tag.id} value={tag.id}>{tag.name}</option>)}</select>
      <select aria-label={t("tags.type")} value={kind} onChange={(event) => setKind(event.target.value as Kind | "")}><option value="">{t("tags.all")}</option>{(["note", "task", "calendar"] as const).map((kind) => <option key={kind} value={kind}>{t(`result.${kind}`)} ({results?.counts[kind] ?? 0})</option>)}</select>
      <label><input type="checkbox" checked={archived} onChange={(event) => setArchived(event.target.checked)} /> {t("tags.include-archived")}</label>
      <button disabled={!tagId} onClick={() => void edit(false)}>{t("tags.rename")}</button><button disabled={!tagId} onClick={() => void edit(true)}>{t("tags.delete")}</button>
    </div>
    {error && <p role="alert">{error}</p>}
    {results?.unavailable_spaces.length ? <p role="status">{t("tags.partial")}</p> : null}
    {results?.items.map((item) => <a className="flex gap-3 border-b p-3" key={`${item.kind}:${item.route}`} href={item.route}><span>{t(`result.${item.kind}`)}</span><span>{item.title}</span>{item.status && <span>{t(`tags.status-${item.status}` as "tags.status-todo")}</span>}{item.archived && <span>{t("tags.archived")}</span>}</a>)}
    {(!tagId || results?.items.length === 0) && <p>{t("command.empty")}</p>}
  </main>;
}
