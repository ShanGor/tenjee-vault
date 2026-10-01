import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Editor } from "@tiptap/react";
import { usePreferences } from "../../i18n";
import { taskApi } from "../tasks/api";

type Link = { task_id: string; node_id: string; state: "none" | "missing" | "locked" | "linked"; route: string | null };
type Todo = { nodeId: string; taskId: string | null; title: string };

export function TaskSource({ taskId }: { taskId: string }) {
  const { t, formatError } = usePreferences();
  const [link, setLink] = useState<Link | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    const refresh = () => { setLink(null); void invoke<Link>("inspect_note_task", { taskId }).then((next) => { if (active) setLink(next); }).catch((reason) => { if (active) setError(formatError(reason)); }); };
    refresh(); const pending = listen("section-locked", refresh);
    return () => { active = false; void pending.then((dispose) => dispose()); };
  }, [taskId, formatError]);
  async function unlink() {
    try { await invoke("unlink_note_task", { taskId }); setLink(null); }
    catch (reason) { setError(formatError(reason)); }
  }
  return <div className="my-3 text-sm">{link && link.state !== "none" && <>
    {link.route ? <a href={link.route}>{t("links.open-note")}</a> : <span>{t(link.state === "locked" ? "links.locked" : "links.missing")}</span>}
    <button className="ml-3" disabled={link.state === "locked"} onClick={() => void unlink()}>{t("links.unlink")}</button>
  </>}{error && <p role="alert">{error}</p>}</div>;
}

export function NoteTasks({ editor, spaceId, pageId, beforeSave, refreshPage }: { editor: Editor; spaceId: string; pageId: string; beforeSave(): Promise<void>; refreshPage(): Promise<void> }) {
  const { t, formatError } = usePreferences();
  const [todos, setTodos] = useState<Todo[]>([]);
  const [lists, setLists] = useState<{id: string; name: string}[]>([]);
  const [listId, setListId] = useState("");
  const [links, setLinks] = useState<Record<string, Link>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    const update = () => {
      const next: Todo[] = [];
      editor.state.doc.descendants((node) => { if (node.type.name === "taskItem" && node.attrs.nodeId) next.push({ nodeId: node.attrs.nodeId, taskId: node.attrs.taskId, title: node.textContent }); });
      setTodos(next);
    };
    update(); editor.on("update", update);
    return () => { editor.off("update", update); };
  }, [editor, pageId]);
  useEffect(() => {
    let active = true;
    void taskApi.lists().then((next) => { if (active) { setLists(next); setListId(next[0]?.id ?? ""); } }).catch((reason) => { if (active) setError(formatError(reason)); });
    return () => { active = false; };
  }, [pageId]);
  useEffect(() => {
    let active = true;
    void Promise.all(todos.filter((todo) => todo.taskId).map(async (todo) => [todo.taskId!, await invoke<Link>("inspect_note_task", { taskId: todo.taskId })] as const))
      .then((entries) => { if (active) setLinks(Object.fromEntries(entries)); }).catch((reason) => { if (active) setError(formatError(reason)); });
    return () => { active = false; };
  }, [todos, formatError]);
  async function act(todo: Todo) {
    setBusy(true); setError("");
    const editable = editor.isEditable;
    editor.setEditable(false);
    try {
      await beforeSave();
      if (todo.taskId) {
        await invoke("unlink_note_task", { taskId: todo.taskId });
        // Deleted tasks have no source record; remove the stale reference in this document.
        const transaction = editor.state.tr;
        editor.state.doc.descendants((node, pos) => { if (node.type.name === "taskItem" && node.attrs.nodeId === todo.nodeId) transaction.setNodeMarkup(pos, undefined, { ...node.attrs, taskId: null }); });
        editor.view.dispatch(transaction);
        await beforeSave();
      } else await invoke("link_note_task", { spaceId, pageId, nodeId: todo.nodeId, listId });
      await refreshPage();
    } catch (reason) { setError(formatError(reason)); }
    finally { editor.setEditable(editable); setBusy(false); }
  }
  if (!todos.length) return null;
  return <details className="mx-2 rounded border p-2"><summary>{t("links.title")}</summary>
    <label>{t("quick.task-list")} <select value={listId} onChange={(event) => setListId(event.target.value)} disabled={busy}>{lists.map((list) => <option key={list.id} value={list.id}>{list.name}</option>)}</select></label>
    {todos.map((todo) => <div className="flex flex-wrap gap-3 py-1" key={todo.nodeId}><span className="flex-1">{todo.title}</span>
      {todo.taskId && (links[todo.taskId]?.state === "linked" ? <a href={`#/tasks?task=${todo.taskId}`}>{t("links.open-task")}</a> : <span>{t("links.missing")}</span>)}
      <button disabled={busy || (!todo.taskId && !listId)} onClick={() => void act(todo)}>{t(todo.taskId ? "links.unlink" : "links.create")}</button>
    </div>)}{error && <p role="alert">{error}</p>}
  </details>;
}
