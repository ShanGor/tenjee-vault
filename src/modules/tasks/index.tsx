import { Icon } from "../../shared/Icon";
import { ListDialog, MedicationCourseDialog, TaskCreateDialog } from "./TaskDialogs";
import { ui, uiError } from "../../i18n/ui";
import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useEffect, useMemo, useState } from "react";
import { usePointerDrag } from "../../shared";
import { formatDate } from "../../shared/date";
import { bytesToBase64, taskApi, Task, TaskList, TaskNode, TaskSearchHit, TaskStatus } from "./api";
import { useTaskStore } from "./store";
import { readViewState, writeViewState } from "../../shared/viewState";

type View = "list" | "kanban" | "today" | "week" | "overdue" | "archive" | "search";
type SavedTaskView = { view: View; filter: "active" | "all" | "completed"; query: string; includeArchived: boolean };
const defaultTaskView: SavedTaskView = { view: "list", filter: "active", query: "", includeArchived: false };
function loadTaskView(): SavedTaskView {
  const saved = readViewState<Partial<SavedTaskView>>("tasks-view", {});
  return { ...defaultTaskView, ...saved,
    view: ["list", "kanban", "today", "week", "overdue", "archive", "search"].includes(saved.view ?? "") ? saved.view! : "list",
    filter: ["active", "all", "completed"].includes(saved.filter ?? "") ? saved.filter! : "active" };
}

export function TasksApp() {
  const labels: Record<View, string> = { list: ui("任务"), kanban: ui("看板"), today: ui("今天"), week: ui("本周"), overdue: ui("逾期"), archive: ui("归档"), search: ui("搜索") };
  const store = useTaskStore();
  const [listDialog, setListDialog] = useState<{ value?: TaskList } | null>(null);
  const [taskDialog, setTaskDialog] = useState<{ parent?: Task } | null>(null);
  const [medicationDialog, setMedicationDialog] = useState(false);
  const [selectionMode, setSelectionMode] = useState(false);
  const [actionError, setActionError] = useState("");
  const [quickSaving, setQuickSaving] = useState(false);
  const [savedView] = useState(loadTaskView);
  const [view, setView] = useState<View>(savedView.view);
  const [quickTitle, setQuickTitle] = useState("");
  const [flatTasks, setFlatTasks] = useState<Task[]>([]);
  const [searchHits, setSearchHits] = useState<TaskSearchHit[]>([]);
  const [query, setQuery] = useState(savedView.query);
  const [includeArchived, setIncludeArchived] = useState(savedView.includeArchived);
  const [taskFilter, setTaskFilter] = useState<"active" | "all" | "completed">(savedView.filter);
  useEffect(() => { writeViewState("tasks-view", { view, filter: taskFilter, query, includeArchived }); }, [view, taskFilter, query, includeArchived]);
  useEffect(() => { void (async () => {
    await store.loadLists();
    const taskId = new URLSearchParams((window.location.hash.split("?")[1] ?? "")).get("task");
    if (!taskId) return;
    for (const list of useTaskStore.getState().lists) {
      const tree = await taskApi.listView(list.id);
      if (flatten(tree).some((task) => task.id === taskId)) {
        await useTaskStore.getState().selectList(list.id);
        useTaskStore.setState({ selectedTaskId: taskId });
        break;
      }
    }
  })(); }, []);
  useEffect(() => { void loadFlat(view, setFlatTasks); }, [view]);

  const allTasks = useMemo(() => flatten(store.tree), [store.tree]);
  const selectedTask = [...allTasks, ...flatTasks].find((task) => task.id === store.selectedTaskId) ?? null;
  async function refresh() { await store.refresh(); await loadFlat(view, setFlatTasks); }
  async function quickAdd() {
    if (!quickTitle.trim() || !store.selectedListId) return;
    if (quickSaving) return;
    setQuickSaving(true); setActionError("");
    try { await taskApi.create(store.selectedListId, quickTitle.trim()); setQuickTitle(""); await store.refresh(); }
    catch (reason) { setActionError(uiError(reason)); } finally { setQuickSaving(false); }
  }

  return <div className="flex h-full text-neutral-900 dark:text-neutral-100">
    <aside className="flex w-60 shrink-0 flex-col border-r">
      <div className="flex items-center justify-between p-3"><b>{ui("任务列表")}</b><button className="task-icon-button" aria-label={ui("新建列表")} title={ui("新建列表")} onClick={() => setListDialog({})}><Icon name="plus" /></button></div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2">{store.lists.map((list, index) =>
        <button key={list.id} draggable data-list-id={list.id}
          onDragStart={(e) => e.dataTransfer.setData("application/x-list-index", String(index))}
          onDragOver={(e) => e.preventDefault()}
          onDrop={async (e) => { e.preventDefault(); const rawIndex = e.dataTransfer.getData("application/x-list-index"); if (rawIndex !== "") { const from = Number(rawIndex); const ids = store.lists.map((item) => item.id); const [moved] = ids.splice(from, 1); ids.splice(index, 0, moved); await taskApi.reorderLists(ids); await store.loadLists(); } else { const id = e.dataTransfer.getData("application/x-task-id"); if (id) { await taskApi.update(id, { list_id: list.id }); await refresh(); } } }}
          onClick={() => { setView("list"); setSelectionMode(false); void store.selectList(list.id); }}
          onContextMenu={(event) => { event.preventDefault(); setListDialog({ value: list }); }}
          className={`mb-1 flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm ${store.selectedListId === list.id && view === "list" ? "bg-blue-100 dark:bg-blue-900" : "hover:bg-neutral-100 dark:hover:bg-neutral-800"}`}>
          <i className="h-2.5 w-2.5 rounded-full" style={{ background: list.color ?? "#94a3b8" }} />{list.name}
        </button>)}</div>
      <nav className="space-y-1 border-t p-2 text-sm">{(["kanban", "today", "week", "overdue", "archive", "search"] as View[]).map((item) => <button key={item} onClick={() => { setView(item); setSelectionMode(false); useTaskStore.setState({ selected: new Set() }); }} className={`block w-full rounded px-2 py-1 text-left ${view === item ? "bg-blue-100 dark:bg-blue-900" : ""}`}>{labels[item]}</button>)}</nav>
    </aside>
    <main className="flex min-w-0 flex-1 flex-col">
      <header className="task-page-header flex items-center gap-2 border-b p-3"><div><h2 className="font-semibold">{view === "list" ? store.lists.find((item) => item.id === store.selectedListId)?.name ?? ui("任务") : labels[view]}</h2>{view === "list" && store.lists.find((item) => item.id === store.selectedListId)?.name === "收件箱" && <p className="task-list-hint">{ui("收件箱是默认任务列表，用来暂存尚未分类的任务。")}</p>}</div><div className="ml-auto flex items-center gap-2">{view === "list" && <><button className="rounded border px-3 py-2 text-xs" onClick={() => setListDialog({ value: store.lists.find((item) => item.id === store.selectedListId) })}>{ui("编辑列表")}</button><button className="rounded border px-3 py-2 text-xs" aria-pressed={selectionMode} onClick={() => { setSelectionMode(!selectionMode); useTaskStore.setState({ selected: new Set() }); }}>{ui(selectionMode ? "退出选择" : "批量选择")}</button></>}<button className="rounded border px-3 py-2 text-xs" disabled={!store.selectedListId} onClick={() => setMedicationDialog(true)}>{ui("服药疗程")}</button><button className="primary-button" disabled={!store.selectedListId} onClick={() => setTaskDialog({})}><Icon name="plus" size={16} />{ui("新建任务")}</button></div></header>
      {selectionMode && <div className="task-selection-bar"><span>{ui("已选择 {p0} 项", { p0: store.selected.size })}</span><BatchToolbar refresh={refresh} /></div>}
      {actionError && <p role="alert" className="task-form-error px-6">{actionError}</p>}
      {view === "list" && <><div className="flex gap-2 border-b p-3"><input className="flex-1 rounded border px-3 py-1.5 dark:bg-neutral-900" placeholder={ui("快速添加任务")} value={quickTitle} onChange={(e) => setQuickTitle(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void quickAdd()} /><button disabled={quickSaving || !quickTitle.trim()} className="rounded bg-blue-600 px-3 text-white" onClick={quickAdd}>{ui("添加")}</button></div><div className="flex items-center gap-1 border-b px-3 py-2">{([{ id: "active", label: ui("未完成") }, { id: "all", label: ui("全部任务") }, { id: "completed", label: ui("仅已完成") }] as const).map((item) => <button key={item.id} aria-pressed={taskFilter === item.id} className={`rounded px-3 py-1 text-xs ${taskFilter === item.id ? "bg-blue-100 text-blue-800 dark:bg-blue-950 dark:text-blue-200" : "text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-800"}`} onClick={() => setTaskFilter(item.id)}>{item.label}</button>)}</div><div className="min-h-0 flex-1 overflow-y-auto p-3">{filterTree(store.tree, taskFilter).map((node, index) => <TaskRow key={node.id} node={node} depth={0} index={index} selectionMode={selectionMode} onCreateChild={(parent) => setTaskDialog({ parent })} onRefresh={store.refresh} />)}</div></>}
      {view === "kanban" && <Kanban tasks={flatTasks} onRefresh={refresh} />}
      {(["today", "week", "overdue"] as View[]).includes(view) && <TaskFlatList tasks={flatTasks} onRefresh={refresh} />}
      {view === "archive" && <ArchiveView tasks={flatTasks} onRefresh={refresh} />}
      {view === "search" && <SearchView query={query} setQuery={setQuery} includeArchived={includeArchived} setIncludeArchived={setIncludeArchived} hits={searchHits} search={async () => setSearchHits(query.trim() ? await taskApi.search(query, includeArchived) : [])} />}
    </main>
    {listDialog && <ListDialog value={listDialog.value} onClose={() => setListDialog(null)} onDeleted={async () => { await store.loadLists(); useTaskStore.setState({ selectedTaskId: null, selected: new Set() }); setView("list"); }} onSaved={async (id) => { await store.loadLists(); await store.selectList(id); setView("list"); }} />}
    {taskDialog && <TaskCreateDialog lists={store.lists} listId={store.selectedListId ?? ""} parent={taskDialog.parent} onClose={() => setTaskDialog(null)} onSaved={async (id, listId) => { await store.selectList(listId); await loadFlat(view, setFlatTasks); setView("list"); useTaskStore.setState({ selectedTaskId: id }); }} />}
    {medicationDialog && <MedicationCourseDialog lists={store.lists} listId={store.selectedListId ?? ""} onClose={() => setMedicationDialog(false)} onSaved={async (id, listId) => { await store.selectList(listId); await loadFlat("list", setFlatTasks); setView("list"); if (id) useTaskStore.setState({ selectedTaskId: id }); }} />}
    {selectedTask && <TaskDetails task={selectedTask} onClose={() => useTaskStore.setState({ selectedTaskId: null })} onRefresh={refresh} />}
  </div>;
}

async function loadFlat(view: View, set: (tasks: Task[]) => void) { if (view === "kanban") set(await taskApi.kanban()); else if (["today", "week", "overdue"].includes(view)) set(await taskApi.smart(view as "today" | "week" | "overdue")); else if (view === "archive") set(await taskApi.archived()); }
function flatten(nodes: TaskNode[]): TaskNode[] { return nodes.flatMap((node) => [node, ...flatten(node.children)]); }
function filterTree(nodes: TaskNode[], filter: "active" | "all" | "completed"): TaskNode[] {
  if (filter === "all") return nodes;
  return nodes.flatMap((node) => {
    const children = filterTree(node.children, filter);
    const matches = filter === "completed" ? node.status === "done" : node.status !== "done";
    if (!matches && !children.length) return [];
    if (filter === "completed" && !matches) return children;
    return [{ ...node, children }];
  });
}
function dueDateClass(date: string | null | undefined) {
  if (!date) return "text-neutral-400";
  const current = formatDate(new Date());
  if (date < current) return "font-medium text-red-600 dark:text-red-400";
  if (date === current) return "font-medium text-blue-600 dark:text-blue-400";
  return "text-neutral-400";
}

function BatchToolbar({ refresh }: { refresh(): Promise<void> }) {
  const selected = useTaskStore((state) => state.selected); const lists = useTaskStore((state) => state.lists);
  if (!selected.size) return null;
  const run = async (action: string, list: string | null = null) => { await taskApi.batch([...selected], action, list); useTaskStore.setState({ selected: new Set() }); await refresh(); };
  return <div className="ml-auto flex gap-1 text-sm">{[[ui("完成"), "complete"], [ui("取消"), "cancel"], [ui("删除"), "delete"]].map(([label, action]) => <button className="rounded border px-2 py-1" key={action} onClick={() => run(action)}>{label}</button>)}<select defaultValue="" onChange={(e) => e.target.value && void run("move", e.target.value)}><option value="">{ui("移动到…")}</option>{lists.map((list) => <option key={list.id} value={list.id}>{list.name}</option>)}</select></div>;
}

function TaskRow({ node, depth, index, selectionMode, onCreateChild, onRefresh }: { node: TaskNode; depth: number; index: number; selectionMode: boolean; onCreateChild(parent: Task): void; onRefresh(): Promise<void> }) {
  const selected = useTaskStore((state) => state.selected);
  const drag = usePointerDrag<{ id: string; parent: string | null; index: number }>({ onEnd: async ({ data, delta }) => { const shift = Math.round(delta.y / 44); if (shift) { await taskApi.reorder(data.id, data.parent, Math.max(0, data.index + shift)); await onRefresh(); } } });
  return <><div draggable onDragStart={(e) => { e.dataTransfer.setData("application/x-task-id", node.id); e.dataTransfer.setData("application/x-task", JSON.stringify({ id: node.id, title: node.title })); }} className="group flex items-center gap-2 rounded px-2 py-1.5 hover:bg-neutral-50 dark:hover:bg-neutral-900" style={{ paddingLeft: `${depth * 20 + 8}px` }} onPointerMove={drag.onPointerMove} onPointerUp={drag.onPointerUp} onPointerCancel={drag.onPointerCancel}>
    <span className="cursor-grab select-none" onPointerDown={drag.onPointerDown({ id: node.id, parent: node.parent_task_id, index })}>⠿</span>{selectionMode ? <input type="checkbox" aria-label={ui("选择任务：{p0}", {p0: node.title})} checked={selected.has(node.id)} onChange={() => useTaskStore.getState().toggleSelected(node.id)} /> : <input type="checkbox" aria-label={ui("完成任务：{p0}", {p0: node.title})} checked={node.status === "done"} onChange={async () => { await taskApi.setStatus(node.id, node.status === "done" ? "todo" : "done"); await onRefresh(); }} />}<button className={`min-w-0 flex-1 truncate text-left ${node.status === "done" ? "line-through text-neutral-400" : ""}`} onClick={() => useTaskStore.setState({ selectedTaskId: node.id })}>{node.title}</button>{node.due_date && <time className={`text-xs ${dueDateClass(node.due_date)}`}>{node.due_date}</time>}<button className="task-icon-button invisible group-hover:visible" title={ui("新建子任务")} aria-label={ui("新建子任务")} onClick={() => onCreateChild(node)}><Icon name="plus" size={16} /></button>
  </div>{node.children.map((child, childIndex) => <TaskRow key={child.id} node={child} depth={depth + 1} index={childIndex} selectionMode={selectionMode} onCreateChild={onCreateChild} onRefresh={onRefresh} />)}</>;
}

function TaskFlatList({ tasks, onRefresh }: { tasks: Task[]; onRefresh(): Promise<void> }) { return <div className="overflow-y-auto p-3">{tasks.map((task) => <div className="flex items-center gap-2 border-b p-2" key={task.id}><input type="checkbox" aria-label={ui("完成任务：{p0}", {p0: task.title})} checked={task.status === "done"} onChange={async () => { await taskApi.setStatus(task.id, task.status === "done" ? "todo" : "done"); await onRefresh(); }} /><button className="flex-1 text-left" onClick={() => useTaskStore.setState({ selectedTaskId: task.id })}>{task.title}</button><span className="text-xs text-neutral-400">{task.due_date}</span></div>)}</div>; }
const taskStatuses = (): { id: TaskStatus; label: string }[] => [{ id: "todo", label: ui("待办") }, { id: "in_progress", label: ui("进行中") }, { id: "done", label: ui("完成") }, { id: "cancelled", label: ui("取消") }];
function Kanban({ tasks, onRefresh }: { tasks: Task[]; onRefresh(): Promise<void> }) { return <div className="grid min-h-0 flex-1 grid-cols-4 gap-3 overflow-x-auto p-3">{taskStatuses().map((column) => <section key={column.id} className="rounded-lg bg-neutral-100 p-2 dark:bg-neutral-900" onDragOver={(e) => e.preventDefault()} onDrop={async (e) => { const id = e.dataTransfer.getData("text/task-id"); if (id) { await taskApi.setStatus(id, column.id); await onRefresh(); } }}><h3 className="mb-2 font-semibold">{column.label}</h3>{tasks.filter((task) => task.status === column.id).map((task) => <button draggable onDragStart={(e) => { e.dataTransfer.setData("text/task-id", task.id); e.dataTransfer.setData("application/x-task", JSON.stringify({ id: task.id, title: task.title })); }} onClick={() => useTaskStore.setState({ selectedTaskId: task.id })} className="mb-2 block w-full rounded bg-white p-3 text-left shadow-sm dark:bg-neutral-800" key={task.id}>{task.title}</button>)}</section>)}</div>; }
function ArchiveView({ tasks, onRefresh }: { tasks: Task[]; onRefresh(): Promise<void> }) { return <div className="p-4">{tasks.map((task) => <div className="flex border-b p-2" key={task.id}><span className="flex-1">{task.title}</span><button onClick={async () => { await taskApi.restore(task.id); await onRefresh(); }}>{ui("恢复")}</button><button className="ml-2 text-red-600" onClick={async () => { if (confirm(ui("彻底删除？"))) { await taskApi.purge(task.id); await onRefresh(); } }}>{ui("删除")}</button></div>)}</div>; }
function SearchView({ query, setQuery, includeArchived, setIncludeArchived, hits, search }: { query: string; setQuery(v: string): void; includeArchived: boolean; setIncludeArchived(v: boolean): void; hits: TaskSearchHit[]; search(): Promise<void> }) { return <div className="p-4"><div className="mb-4 flex gap-2"><input className="flex-1 rounded border px-3 py-2 dark:bg-neutral-900" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void search()} placeholder={ui("搜索标题与备注")} /><label className="flex items-center gap-1 text-sm"><input type="checkbox" checked={includeArchived} onChange={(e) => setIncludeArchived(e.target.checked)} />{ui("含归档")}</label><button onClick={search}>{ui("搜索")}</button></div>{hits.map((hit) => <button key={hit.task_id} onClick={() => useTaskStore.setState({ selectedTaskId: hit.task_id })} className="block w-full border-b p-3 text-left"><b>{hit.title}</b><div className="text-sm text-neutral-500" dangerouslySetInnerHTML={{ __html: hit.snippet }} /></button>)}</div>; }

import { TagSelector } from "../../shared/Tags";
import { TaskSource } from "../notes/NoteTasks";

function TaskDetails({ task, onClose, onRefresh }: { task: Task; onClose(): void; onRefresh(): Promise<void> }) {
  const [draft, setDraft] = useState(task); const [attachments, setAttachments] = useState<Awaited<ReturnType<typeof taskApi.attachments>>>([]);
  useEffect(() => { setDraft(task); }, [task]);
  useEffect(() => { void taskApi.attachments(task.id).then(setAttachments); }, [task.id]);
  async function save(patch: Parameters<typeof taskApi.update>[1]) { await taskApi.update(task.id, patch); await onRefresh(); }
  return <aside className="w-96 shrink-0 overflow-y-auto border-l bg-white p-4 dark:bg-neutral-950"><div className="mb-3 flex"><b className="flex-1">{ui("任务详情")}</b><button onClick={onClose}>×</button></div><input className="mb-3 w-full border-b bg-transparent text-lg font-semibold" aria-label={ui("标题")} value={draft.title} onChange={(e) => setDraft({ ...draft, title: e.target.value })} onBlur={() => save({ title: draft.title })} />
    <div className="grid grid-cols-2 gap-2 text-sm"><label className="task-field"><span>{ui("状态")}</span><select aria-label={ui("状态")} value={draft.status} onChange={async (e) => { await taskApi.setStatus(task.id, e.target.value as TaskStatus); await onRefresh(); }}><option value="todo">{ui("待办")}</option><option value="in_progress">{ui("进行中")}</option><option value="done">{ui("完成")}</option><option value="cancelled">{ui("取消")}</option></select></label><label className="task-field"><span>{ui("优先级")}</span><select aria-label={ui("优先级")} value={draft.priority} onChange={(e) => { const priority = e.target.value as Task["priority"]; setDraft({ ...draft, priority }); void save({ priority }); }}><option value="none">{ui("无优先级")}</option><option value="low">{ui("低")}</option><option value="medium">{ui("中")}</option><option value="high">{ui("高")}</option></select></label><label className="task-field"><span>{ui("截止日期")}</span><input type="date" value={draft.due_date ?? ""} onChange={(e) => { const value = e.target.value; setDraft({ ...draft, due_date: value || null }); void save(value ? { due_date: value } : { clear_due_date: true }); }} /></label><label className="task-field"><span>{ui("截止时间")}</span><input type="time" value={draft.due_time ?? ""} onChange={(e) => { const value = e.target.value; setDraft({ ...draft, due_time: value || null }); void save(value ? { due_time: value } : { clear_due_time: true }); }} /></label><label className="task-field task-field-wide"><span>{ui("提醒时间")}</span><input  type="datetime-local" value={draft.reminder_at?.slice(0, 16) ?? ""} onChange={(e) => { const value = e.target.value ? `${e.target.value}:00` : ""; setDraft({ ...draft, reminder_at: value || null }); void save(value ? { reminder_at: value } : { clear_reminder_at: true }); }} /></label><label className="task-field task-field-wide"><span>{ui("重复规则")}</span><input className="rounded border px-2 py-1" placeholder={ui("RRULE，例如 FREQ=WEEKLY")} value={draft.recurrence_rule ?? ""} onChange={(e) => setDraft({ ...draft, recurrence_rule: e.target.value || null })} onBlur={() => save(draft.recurrence_rule ? { recurrence_rule: draft.recurrence_rule } : { clear_recurrence_rule: true })} /></label></div>
    <label className="mt-4 block text-xs text-neutral-500">{ui("备注")}</label><TaskNotes task={draft} onSave={(notes) => save({ notes })} /><TagSelector key={task.id} kind="task" id={task.id} /><TaskSource key={`source:${task.id}`} taskId={task.id} />
    <div className="mt-4"><div className="flex"><b className="flex-1 text-sm">{ui("附件")}</b><button onClick={() => pickFile(task.id, async () => setAttachments(await taskApi.attachments(task.id)))}>＋</button></div>{attachments.map((attachment) => <div className="flex text-sm" key={attachment.id}><button className="flex-1 truncate text-left" onClick={() => openAttachment(attachment.id, attachment.mime)}>{attachment.file_name}</button><button onClick={async () => { await taskApi.deleteAttachment(attachment.id); setAttachments(await taskApi.attachments(task.id)); }}>×</button></div>)}</div>
    <div className="mt-6 flex gap-2"><button className="rounded border px-2 py-1" onClick={async () => { await taskApi.archive([task.id]); onClose(); await onRefresh(); }}>{ui("归档")}</button><button className="rounded border px-2 py-1 text-red-600" onClick={async () => { if (confirm(ui("删除任务？"))) { await taskApi.remove(task.id); onClose(); await onRefresh(); } }}>{ui("删除")}</button></div></aside>;
}
function TaskNotes({ task, onSave }: { task: Task; onSave(notes: string): Promise<void> }) { const editor = useEditor({ extensions: [StarterKit], content: safeJson(task.notes), editorProps: { attributes: { class: "prose-editor min-h-32 rounded border p-2" } }, onBlur: ({ editor }) => { void onSave(JSON.stringify(editor.getJSON())); } }, [task.id]); return <EditorContent editor={editor} />; }
function safeJson(value: string | null) { try { return value ? JSON.parse(value) : ""; } catch { return value ?? ""; } }
function pickFile(taskId: string, done: () => Promise<void>) { const input = document.createElement("input"); input.type = "file"; input.onchange = async () => { const file = input.files?.[0]; if (!file) return; await taskApi.saveAttachment(taskId, file.name, file.type || null, bytesToBase64(new Uint8Array(await file.arrayBuffer()))); await done(); }; input.click(); }
async function openAttachment(id: string, mime: string | null) { const opened = await taskApi.openAttachment(id); const bytes = Uint8Array.from(atob(opened.data_base64), (char) => char.charCodeAt(0)); const url = URL.createObjectURL(new Blob([bytes], { type: mime ?? "application/octet-stream" })); window.open(url); }
