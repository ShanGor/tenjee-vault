import { invoke } from "../../shared/mobileReminders";

export type TaskStatus = "todo" | "in_progress" | "done" | "cancelled";
export interface TaskList { id: string; name: string; color: string | null; sort_order: number }
export interface Task {
  id: string; list_id: string; title: string; notes: string | null; status: TaskStatus;
  priority: "none" | "low" | "medium" | "high"; due_date: string | null; due_time: string | null;
  reminder_at: string | null; recurrence_rule: string | null; parent_task_id: string | null;
  sort_order: number; completed_at: string | null; archived_at: string | null;
}
export interface TaskNode extends Task { children: TaskNode[] }
export interface TaskAttachment { id: string; file_name: string; mime: string | null; size: number; hash: string }
export interface TaskSearchHit { task_id: string; title: string; snippet: string; archived: boolean }
export interface MedicationDose { label: string; time: string }
export type TaskPatch = Partial<{
  title: string; notes: string; clear_notes: boolean; priority: string; due_date: string; clear_due_date: boolean;
  due_time: string; clear_due_time: boolean; reminder_at: string; clear_reminder_at: boolean;
  recurrence_rule: string; clear_recurrence_rule: boolean; list_id: string; parent_task_id: string;
  clear_parent_task_id: boolean;
}>;

export const taskApi = {
  lists: () => invoke<TaskList[]>("list_task_lists"),
  createList: (name: string, color: string | null = null) => invoke<TaskList>("create_task_list", { name, color }),
  renameList: (id: string, name: string) => invoke<void>("rename_task_list", { id, name }),
  setListColor: (id: string, color: string | null) => invoke<void>("set_task_list_color", { id, color }),
  reorderLists: (ids: string[]) => invoke<void>("reorder_task_lists", { ids }),
  deleteList: (id: string, confirmNonEmpty: boolean) => invoke<void>("delete_task_list", { id, confirmNonEmpty }),
  create: (listId: string, title: string, parentTaskId: string | null = null) =>
    invoke<Task>("create_task_cmd", { listId, title, parentTaskId }),
  createMedicationCourse: (input: { listId: string; startDate: string; days: number; taskName: string; medicalDetails: string | null; doses: MedicationDose[] }) =>
    invoke<Task[]>("create_medication_course_tasks", {
      listId: input.listId, startDate: input.startDate, days: input.days,
      taskName: input.taskName, medicalDetails: input.medicalDetails, doses: input.doses,
    }),
  update: (id: string, patch: TaskPatch) => invoke<Task>("update_task_cmd", { id, patch }),
  setStatus: (id: string, status: TaskStatus) => invoke<void>("set_task_status_cmd", { id, status }),
  remove: (id: string) => invoke<void>("delete_task_cmd", { id }),
  reorder: (id: string, newParent: string | null, newSortOrder: number) =>
    invoke<void>("reorder_task_cmd", { id, newParent, newSortOrder }),
  listView: (listId: string) => invoke<TaskNode[]>("task_list_view", { listId }),
  kanban: () => invoke<Task[]>("task_kanban_view"),
  smart: (view: "today" | "week" | "overdue") => invoke<Task[]>("task_smart_view", { view }),
  batch: (ids: string[], action: string, targetListId: string | null = null) =>
    invoke<void>("batch_tasks", { ids, action, targetListId }),
  archive: (ids: string[]) => invoke<void>("archive_tasks_cmd", { ids }),
  archived: () => invoke<Task[]>("list_archived_tasks"),
  restore: (id: string) => invoke<void>("restore_task", { id }),
  purge: (id: string) => invoke<void>("purge_task", { id }),
  tags: (taskId: string) => invoke<string[]>("get_task_tags", { taskId }),
  setTags: (taskId: string, tagIds: string[]) => invoke<void>("set_task_tags", { taskId, tagIds }),
  attachments: (taskId: string) => invoke<TaskAttachment[]>("list_task_attachments", { taskId }),
  saveAttachment: (taskId: string, fileName: string, mime: string | null, dataBase64: string) =>
    invoke<TaskAttachment>("save_task_attachment", { taskId, fileName, mime, dataBase64 }),
  openAttachment: (id: string) => invoke<{ attachment: TaskAttachment; data_base64: string }>("open_task_attachment", { id }),
  deleteAttachment: (id: string) => invoke<void>("delete_task_attachment", { id }),
  search: (query: string, includeArchived: boolean) =>
    invoke<TaskSearchHit[]>("search_tasks_cmd", { query, includeArchived, limit: 50 }),
};

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}
