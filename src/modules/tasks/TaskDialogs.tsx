import { useMobileDismiss } from "../../shared/useMobileDismiss";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { ui, uiError } from "../../i18n/ui";
import { Icon } from "../../shared/Icon";
import { taskApi, type MedicationDose, type Task, type TaskList, type TaskStatus } from "./api";

function TaskDialog({ title, description, children, onClose }: { title: string; description: string; children: ReactNode; onClose(): void }) {
  const heading = useId();
  const panel = useRef<HTMLElement>(null);
  const close = useRef(onClose); close.current = onClose;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    panel.current?.querySelector<HTMLElement>('input, button')?.focus();
    const keyboard = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); close.current(); }
      if (event.key !== "Tab") return;
      const items = Array.from(panel.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex="0"]') ?? []);
      const first = items[0], last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    document.addEventListener("keydown", keyboard);
    return () => { document.removeEventListener("keydown", keyboard); previous?.focus(); };
  }, []);
  useMobileDismiss(onClose);
  return <div className="task-dialog-overlay" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section ref={panel} role="dialog" aria-modal="true" aria-labelledby={heading} className="task-dialog">
      <div className="task-dialog-heading"><span className="task-dialog-icon"><Icon name="tasks" size={24} /></span><div><h2 id={heading}>{title}</h2><p>{description}</p></div></div>
      {children}
    </section>
  </div>;
}

const colors = ["#217568", "#537dba", "#9770af", "#bd7850", "#b75e71", "#778277"];
export function ListDialog({ value, onClose, onSaved, onDeleted }: { value?: TaskList; onClose(): void; onSaved(id: string): Promise<void>; onDeleted(): Promise<void> }) {
  const [name, setName] = useState(value?.name ?? "");
  const [color, setColor] = useState(value?.color ?? colors[0]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [confirmDelete, setConfirmDelete] = useState(false);
  const createdId = useRef<string | null>(value?.id ?? null);
  async function remove() {
    if (!value) return;
    setBusy(true); setError("");
    try { await taskApi.deleteList(value.id, true); await onDeleted(); onClose(); }
    catch (reason) { setError(uiError(reason)); } finally { setBusy(false); }
  }
  async function save() {
    if (busy || confirmDelete) return;
    if (!name.trim()) return setError(ui("请输入列表名称。"));
    setBusy(true); setError("");
    try {
      if (!createdId.current) createdId.current = (await taskApi.createList(name.trim(), color)).id;
      else { await taskApi.renameList(createdId.current, name.trim()); await taskApi.setListColor(createdId.current, color); }
      await onSaved(createdId.current); onClose();
    } catch (reason) { setError(uiError(reason)); } finally { setBusy(false); }
  }
  return <TaskDialog title={ui(value ? "编辑列表" : "新建列表")} description={ui("按项目或生活领域整理任务，例如工作、个人或旅行。")} onClose={() => !busy && onClose()}>
    <form onSubmit={(event) => { event.preventDefault(); void save(); }}>
      <div className="task-dialog-body"><label className="task-field"><span>{ui("列表名称")}</span><input disabled={busy} value={name} onChange={(event) => setName(event.target.value)} placeholder={ui("例如：工作项目")} required maxLength={120} /></label>
        <fieldset className="mt-5"><legend className="task-field-label">{ui("颜色")}</legend><div className="list-color-options">{colors.map((item) => <label key={item} className="list-color-option" style={{ backgroundColor: item }}><input type="radio" name="list-color" value={item} checked={color === item} onChange={() => setColor(item)} disabled={busy} aria-label={`${ui("颜色")} ${item}`} /><span aria-hidden="true">{color === item ? "✓" : ""}</span></label>)}<label className="custom-list-color">{ui("自定义颜色")}<input type="color" aria-label={ui("自定义颜色")} value={color} onChange={(event) => setColor(event.target.value)} disabled={busy} /></label></div></fieldset>
        {confirmDelete && value && <div className="task-delete-confirmation"><p>{ui("删除「{p0}」及其中任务？", { p0: value.name })}</p><p>{ui("此操作无法撤销。")}</p><div className="mt-3 flex gap-2"><button type="button" disabled={busy} className="rounded border px-3 py-2" onClick={() => setConfirmDelete(false)}>{ui("取消")}</button><button type="button" disabled={busy} className="rounded bg-red-600 px-3 py-2 text-white" onClick={() => void remove()}>{ui("删除列表")}</button></div></div>}
        {error && <p role="alert" className="task-form-error">{error}</p>}
      </div><footer className="task-dialog-footer">{value && <button type="button" className="mr-auto text-xs text-red-600" disabled={busy || confirmDelete} onClick={() => setConfirmDelete(true)}>{ui("删除列表")}</button>}<button type="button" className="rounded border px-4 py-2" disabled={busy} onClick={onClose}>{ui("取消")}</button><button className="primary-button" disabled={busy || confirmDelete}>{ui(busy ? "正在保存…" : value ? "保存" : "创建列表")}</button></footer>
    </form>
  </TaskDialog>;
}

export function TaskCreateDialog({ lists, listId, parent, onClose, onSaved }: { lists: TaskList[]; listId: string; parent?: Task; onClose(): void; onSaved(id: string, listId: string): Promise<void> }) {
  const [title, setTitle] = useState("");
  const [destination, setDestination] = useState(parent?.list_id ?? listId);
  const [notes, setNotes] = useState("");
  const [status, setStatus] = useState<TaskStatus>("todo");
  const [priority, setPriority] = useState<Task["priority"]>("none");
  const [dueDate, setDueDate] = useState(""); const [dueTime, setDueTime] = useState("");
  const [reminder, setReminder] = useState(""); const [frequency, setFrequency] = useState("");
  const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  // Retain a created task across retryable update failures, so Save never creates duplicates.
  const createdId = useRef<string | null>(null);
  async function save() {
    if (busy) return;
    if (!title.trim()) return setError(ui("请输入任务标题。"));
    if (!destination) return setError(ui("请选择任务列表。"));
    if ((dueTime || frequency) && !dueDate) return setError(ui("截止时间和重复任务需要先设置截止日期。"));
    setBusy(true); setError("");
    try {
      if (!createdId.current) createdId.current = (await taskApi.create(destination, title.trim(), parent?.id ?? null)).id;
      const id = createdId.current;
      await taskApi.update(id, { title: title.trim(), list_id: destination, priority,
        notes: JSON.stringify({ type: "doc", content: notes.split("\n").map((text) => ({ type: "paragraph", ...(text ? { content: [{ type: "text", text }] } : {}) })) }),
        ...(dueDate ? { due_date: dueDate } : { clear_due_date: true }), ...(dueTime ? { due_time: dueTime } : { clear_due_time: true }),
        ...(reminder ? { reminder_at: `${reminder}:00` } : { clear_reminder_at: true }),
        ...(frequency ? { recurrence_rule: `FREQ=${frequency}` } : { clear_recurrence_rule: true }),
      });
      await taskApi.setStatus(id, status);
      await onSaved(id, destination); onClose();
    } catch (reason) { setError(`${createdId.current ? ui("任务已创建，部分信息未保存。请重试保存以完成设置。") + " " : ""}${uiError(reason)}`); } finally { setBusy(false); }
  }
  return <TaskDialog title={ui(parent ? "新建子任务" : "新建任务")} description={parent ? parent.title : ui("明确下一步，并为任务安排优先级和时间。")} onClose={() => !busy && onClose()}>
    <form onSubmit={(event) => { event.preventDefault(); void save(); }}>
      <fieldset disabled={busy} className="task-dialog-body">
        <label className="task-field"><span>{ui("标题")} <span className="text-red-600">*</span></span><input required value={title} onChange={(event) => setTitle(event.target.value)} placeholder={ui("要完成什么？")} /></label>
        <label className="task-field mt-4"><span>{ui("备注")}</span><textarea rows={3} value={notes} onChange={(event) => setNotes(event.target.value)} placeholder={ui("补充背景、步骤或完成标准…")} /></label>
        <div className="task-form-grid mt-5">
          <label className="task-field"><span>{ui("任务列表")}</span><select aria-label={ui("任务列表")} value={destination} onChange={(event) => setDestination(event.target.value)} disabled={!!parent} required>{lists.map((list) => <option key={list.id} value={list.id}>{list.name}</option>)}</select></label>
          <label className="task-field"><span>{ui("状态")}</span><select aria-label={ui("状态")} value={status} onChange={(event) => setStatus(event.target.value as TaskStatus)}><option value="todo">{ui("待办")}</option><option value="in_progress">{ui("进行中")}</option></select></label>
          <label className="task-field"><span>{ui("优先级")}</span><select aria-label={ui("优先级")} value={priority} onChange={(event) => setPriority(event.target.value as Task["priority"])}><option value="none">{ui("无优先级")}</option><option value="low">{ui("低")}</option><option value="medium">{ui("中")}</option><option value="high">{ui("高")}</option></select></label>
          <label className="task-field"><span>{ui("重复")}</span><select aria-label={ui("重复")} value={frequency} onChange={(event) => setFrequency(event.target.value)}><option value="">{ui("不重复")}</option><option value="DAILY">{ui("每日")}</option><option value="WEEKLY">{ui("每周")}</option><option value="MONTHLY">{ui("每月")}</option><option value="YEARLY">{ui("每年")}</option></select></label>
          <label className="task-field"><span>{ui("截止日期")}</span><input type="date" value={dueDate} onChange={(event) => setDueDate(event.target.value)} /></label>
          <label className="task-field"><span>{ui("截止时间")}</span><input type="time" value={dueTime} onChange={(event) => setDueTime(event.target.value)} /></label>
          <label className="task-field task-field-wide"><span>{ui("提醒时间")}</span><input type="datetime-local" aria-label={ui("提醒时间")} value={reminder} onChange={(event) => setReminder(event.target.value)} /><small>{ui("时间使用本地时区；留空则不设置提醒。")}</small></label>
        </div>
        {error && <p role="alert" className="task-form-error">{error}</p>}
      </fieldset>
      <footer className="task-dialog-footer"><button type="button" className="rounded border px-4 py-2" disabled={busy} onClick={onClose}>{ui("取消")}</button><button className="primary-button" disabled={busy}>{ui(busy ? "正在保存…" : "创建任务")}</button></footer>
    </form>
  </TaskDialog>;
}

export function MedicationCourseDialog({ lists, listId, onClose, onSaved }: { lists: TaskList[]; listId: string; onClose(): void; onSaved(id: string, listId: string): Promise<void> }) {
  const now = new Date();
  const localToday = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  const [destination, setDestination] = useState(listId);
  const [startDate, setStartDate] = useState(localToday);
  const [days, setDays] = useState("7");
  const [taskName, setTaskName] = useState("");
  const [medicalDetails, setMedicalDetails] = useState("");
  const [doses, setDoses] = useState<MedicationDose[]>([
    { label: ui("早上"), time: "08:00" },
    { label: ui("中午"), time: "13:00" },
    { label: ui("晚上"), time: "20:00" },
  ]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function save() {
    if (busy) return;
    const dayCount = Number(days);
    if (!destination) return setError(ui("请选择任务列表。"));
    if (!taskName.trim()) return setError(ui("请输入任务标题。"));
    if (!startDate) return setError(ui("请选择开始日期。"));
    if (!Number.isInteger(dayCount) || dayCount < 1 || dayCount > 365) return setError(ui("疗程天数必须在 1 到 365 天之间。"));
    if (!doses.length || doses.some((dose) => !dose.label.trim() || !dose.time)) return setError(ui("请为每次服药填写名称和时间。"));
    setBusy(true); setError("");
    try {
      const created = await taskApi.createMedicationCourse({
        listId: destination, startDate, days: dayCount,
        taskName: taskName.trim(), medicalDetails: medicalDetails.trim() || null,
        doses: doses.map((dose) => ({ label: dose.label.trim(), time: dose.time })),
      });
      await onSaved(created[0]?.id ?? "", destination);
      onClose();
    } catch (reason) { setError(uiError(reason)); } finally { setBusy(false); }
  }
  const taskCount = Number(days) * doses.length;
  return <TaskDialog title={ui("创建服药疗程")} description={ui("为每次服药创建独立任务，并在设定时间提醒。")} onClose={() => !busy && onClose()}>
    <form onSubmit={(event) => { event.preventDefault(); void save(); }}>
      <fieldset disabled={busy} className="task-dialog-body">
        <div className="task-form-grid">
          <label className="task-field task-field-wide"><span>{ui("任务列表")}</span><select value={destination} onChange={(event) => setDestination(event.target.value)} required>{lists.map((list) => <option key={list.id} value={list.id}>{list.name}</option>)}</select></label>
          <label className="task-field"><span>{ui("开始日期")}</span><input type="date" value={startDate} onChange={(event) => setStartDate(event.target.value)} required /></label>
          <label className="task-field"><span>{ui("疗程天数")}</span><input type="number" min="1" max="365" value={days} onChange={(event) => setDays(event.target.value)} required /></label>
          <label className="task-field task-field-wide"><span>{ui("任务名称")} <span className="text-red-600">*</span></span><input value={taskName} maxLength={120} onChange={(event) => setTaskName(event.target.value)} placeholder={ui("例如：药品名称")} required /></label>
        </div>
        <label className="task-field mt-4"><span>{ui("处方 / 医疗详情（可选）")}</span><textarea rows={3} value={medicalDetails} maxLength={4000} onChange={(event) => setMedicalDetails(event.target.value)} placeholder={ui("补充用药说明或注意事项")} /></label>
        <div className="mt-5 flex items-center"><b className="flex-1 text-sm">{ui("每日服药时间")}</b><button type="button" className="rounded border px-2 py-1 text-xs" disabled={doses.length >= 12} onClick={() => setDoses([...doses, { label: "", time: "" }])}>{ui("添加一次")}</button></div>
        <div className="mt-2 space-y-2">{doses.map((dose, index) => <div className="grid grid-cols-[1fr_1fr_auto] items-end gap-2" key={index}>
          <label className="task-field"><span>{ui("服药名称")}</span><input value={dose.label} maxLength={40} onChange={(event) => setDoses(doses.map((item, itemIndex) => itemIndex === index ? { ...item, label: event.target.value } : item))} required placeholder={ui("例如：早上")}/></label>
          <label className="task-field"><span>{ui("服药时间")}</span><input type="time" value={dose.time} onChange={(event) => setDoses(doses.map((item, itemIndex) => itemIndex === index ? { ...item, time: event.target.value } : item))} required /></label>
          <button type="button" className="h-9 rounded border px-2" aria-label={ui("移除此时间")} disabled={doses.length <= 1} onClick={() => setDoses(doses.filter((_, itemIndex) => itemIndex !== index))}>×</button>
        </div>)}</div>
        <p className="mt-3 text-xs text-neutral-500">{ui("将创建 {p0} 个任务；每个任务会在服药时间提醒。", { p0: Number.isFinite(taskCount) ? taskCount : 0 })}</p>
        {error && <p role="alert" className="task-form-error mt-3">{error}</p>}
      </fieldset>
      <footer className="task-dialog-footer"><button type="button" className="rounded border px-4 py-2" disabled={busy} onClick={onClose}>{ui("取消")}</button><button className="primary-button" disabled={busy}>{ui(busy ? "正在创建…" : "创建疗程任务")}</button></footer>
    </form>
  </TaskDialog>;
}
