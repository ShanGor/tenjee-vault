import { ui, commandErrorCode } from "../../i18n/ui";
// 后端 command 的薄封装：类型定义 + invoke 包装。

export interface SpaceInfo {
  id: string;
  name: string;
  db_file: string;
}

export interface Section {
  id: string;
  notebook_id: string;
  section_group_id: string | null;
  name: string;
  color: string | null;
  sort_order: number;
  is_encrypted: boolean;
  is_unlocked: boolean;
}

export interface SectionGroupNode {
  id: string;
  notebook_id: string;
  parent_group_id: string | null;
  name: string;
  sort_order: number;
  children: SectionGroupNode[];
  sections: Section[];
}

export interface NotebookNode {
  id: string;
  name: string;
  color: string | null;
  sort_order: number;
  groups: SectionGroupNode[];
  sections: Section[];
}

export interface TreeDto {
  notebooks: NotebookNode[];
  unlocked_section_ids: string[];
}

export interface PageNode {
  id: string;
  section_id: string;
  parent_page_id: string | null;
  title: string;
  sort_order: number;
  updated_at: string;
  children: PageNode[];
}

export interface Page {
  id: string;
  section_id: string;
  parent_page_id: string | null;
  title: string;
  content: string;
  created_at: string;
  updated_at: string;
  sort_order: number;
}

export interface PageVersion {
  id: string;
  page_id: string;
  created_at: string;
  content: string;
}

export interface TrashEntry {
  id: string;
  title: string;
  deleted_at: string;
  section_name: string;
  notebook_name: string;
}

export interface RecentEntry {
  space_id: string;
  page_id: string;
  title: string;
  opened_at: string;
  locked: boolean;
}

export interface Attachment {
  id: string;
  entity_type: string;
  entity_id: string;
  file_name: string;
  mime: string | null;
  size: number;
  hash: string;
  created_at: string;
}

export interface AttachmentData extends Attachment {
  data_base64: string;
}

export interface SearchResult {
  page_id: string;
  section_id: string;
  section_name: string;
  notebook_name: string;
  title: string;
  snippet: string;
  from_unlocked: boolean;
}

export interface PageTitle {
  id: string;
  title: string;
  section_id: string;
}

export interface AppSettings {
  clipboard_auto_clear_seconds: number;
  section_auto_lock_minutes: number;
  encrypted_section_show_titles: boolean;
  default_space_name: string;
  lunar_overlay_enabled: boolean;
  festivals_enabled: boolean;
  solar_terms_enabled: boolean;
  auto_backup_enabled: boolean;
  auto_backup_schedule: "daily" | "weekly";
  auto_backup_directory: string;
  auto_backup_retention_count: number;
  auto_backup_last_success_at: string;
  close_to_tray: boolean;
  app_locale: "system" | "zh-CN" | "en";
  app_theme: "system" | "light" | "dark";
  app_shortcuts: string;
}

export interface BackupSummary {
  path: string;
  files: number;
  total_bytes: number;
  managed_auto_backup: boolean;
}

export interface BackupVerification {
  files: number;
  app_version: string;
  created_at: string;
}
export interface RestoreDiagnostic { nonce: string; failed_candidate: string; message: string; occurred_at: string; }
export interface GlobalSearchHit { kind: "note" | "task" | "calendar"; id: string; title: string; snippet: string; context: string; score: number; route: string; }

export type NoteExportFormat = "markdown" | "html" | "pdf";
export interface BatchItem<T> {
  item: string;
  status: "success" | "skipped" | "failed";
  value: T | null;
  reason: string | null;
}
export interface BatchResult<T> { items: BatchItem<T>[]; }
export interface ImportedPage { page_id: string; title: string; attachment_count: number; }

export class LockedError extends Error {}

function isLocked(e: unknown): boolean {
  return commandErrorCode(e) === "section_locked";
}

export function rethrow(e: unknown): never {
  if (isLocked(e)) throw new LockedError(typeof e === "string" ? e : ui("分区已锁定"));
  throw e;
}

import { invoke } from "@tauri-apps/api/core";

export const api = {
  // 空间
  listSpaces: () => invoke<SpaceInfo[]>("list_spaces"),
  createSpace: (name: string) => invoke<SpaceInfo>("create_space_cmd", { name }),
  renameSpace: (id: string, name: string) => invoke<void>("rename_space_cmd", { id, name }),
  archiveSpace: (id: string) => invoke<void>("archive_space_cmd", { id }),
  deleteSpace: (id: string) => invoke<void>("delete_space_cmd", { id }),
  ensureDefaultSpace: () => invoke<SpaceInfo[]>("ensure_default_space"),

  // 设置
  getSettings: () => invoke<AppSettings>("get_settings"),
  setSetting: (key: string, value: string) => invoke<AppSettings>("set_setting_cmd", { key, value }),
  resetSettings: (group: string) => invoke<AppSettings>("reset_settings_group", { group }),

  // 备份与恢复
  createBackup: (directory: string, filename: string, overwrite = false) =>
    invoke<BackupSummary>("create_backup_cmd", { directory, filename, overwrite }),
  verifyBackup: (directory: string, filename: string) =>
    invoke<BackupVerification>("verify_backup_cmd", { directory, filename }),
  prepareRestore: (directory: string, filename: string) =>
    invoke<string>("prepare_restore_cmd", { directory, filename }),
  restoreDiagnostic: () => invoke<RestoreDiagnostic | null>("get_restore_diagnostic_cmd"),
  clearPreRestoreCopies: () => invoke<{ removed: number }>("clear_pre_restore_copies_cmd"),
  globalSearch: (query: string, limit = 30) => invoke<GlobalSearchHit[]>("global_search_cmd", { query, limit }),

  // 导航树
  getTree: (spaceId: string) => invoke<TreeDto>("get_tree", { spaceId }),
  getPageTree: (spaceId: string, sectionId: string) =>
    invoke<PageNode[]>("get_page_tree", { spaceId, sectionId }),
  listPageTitles: (spaceId: string) => invoke<PageTitle[]>("list_page_titles_cmd", { spaceId }),

  // 笔记本
  createNotebook: (spaceId: string, name: string, color: string | null) =>
    invoke<NotebookNode>("create_notebook_cmd", { spaceId, name, color }),
  renameNotebook: (spaceId: string, id: string, name: string) =>
    invoke<void>("rename_notebook_cmd", { spaceId, id, name }),
  setNotebookColor: (spaceId: string, id: string, color: string | null) =>
    invoke<void>("set_notebook_color_cmd", { spaceId, id, color }),
  reorderNotebooks: (spaceId: string, ids: string[]) =>
    invoke<void>("reorder_notebooks_cmd", { spaceId, ids }),
  deleteNotebook: (spaceId: string, id: string) => invoke<void>("delete_notebook_cmd", { spaceId, id }),

  // 分区组
  createSectionGroup: (spaceId: string, notebookId: string, parentGroupId: string | null, name: string) =>
    invoke<SectionGroupNode>("create_section_group_cmd", { spaceId, notebookId, parentGroupId, name }),
  renameSectionGroup: (spaceId: string, id: string, name: string) =>
    invoke<void>("rename_section_group_cmd", { spaceId, id, name }),
  moveSectionGroup: (spaceId: string, id: string, notebookId: string, parentGroupId: string | null) =>
    invoke<void>("move_section_group_cmd", { spaceId, id, notebookId, parentGroupId }),
  deleteSectionGroup: (spaceId: string, id: string) =>
    invoke<void>("delete_section_group_cmd", { spaceId, id }),

  // 分区
  createSection: (spaceId: string, notebookId: string, sectionGroupId: string | null, name: string, color: string | null) =>
    invoke<Section>("create_section_cmd", { spaceId, notebookId, sectionGroupId, name, color }),
  renameSection: (spaceId: string, id: string, name: string) =>
    invoke<void>("rename_section_cmd", { spaceId, id, name }),
  setSectionColor: (spaceId: string, id: string, color: string | null) =>
    invoke<void>("set_section_color_cmd", { spaceId, id, color }),
  moveSection: (spaceId: string, id: string, notebookId: string, sectionGroupId: string | null) =>
    invoke<void>("move_section_cmd", { spaceId, id, notebookId, sectionGroupId }),
  reorderSections: (spaceId: string, ids: string[]) =>
    invoke<void>("reorder_sections_cmd", { spaceId, ids }),
  deleteSection: (spaceId: string, id: string) => invoke<void>("delete_section_cmd", { spaceId, id }),

  // 页面
  createPage: (spaceId: string, sectionId: string, parentPageId: string | null, title: string) =>
    invoke<PageNode>("create_page_cmd", { spaceId, sectionId, parentPageId, title }),
  renamePage: (spaceId: string, id: string, title: string) =>
    invoke<void>("rename_page_cmd", { spaceId, id, title }),
  movePage: (spaceId: string, id: string, sectionId: string, parentPageId: string | null, sortOrder: number) =>
    invoke<void>("move_page_cmd", { spaceId, id, sectionId, parentPageId, sortOrder }),
  deletePage: (spaceId: string, id: string) => invoke<void>("delete_page_cmd", { spaceId, id }),
  restorePage: (spaceId: string, id: string) => invoke<void>("restore_page_cmd", { spaceId, id }),
  purgePage: (spaceId: string, id: string) => invoke<void>("purge_page_cmd", { spaceId, id }),
  listTrash: (spaceId: string) => invoke<TrashEntry[]>("list_trash_cmd", { spaceId }),
  getPage: (spaceId: string, pageId: string) => invoke<Page>("get_page_cmd", { spaceId, pageId }),
  savePage: (spaceId: string, pageId: string, title: string, content: string) =>
    invoke<void>("save_page_cmd", { spaceId, pageId, title, content }),
  listVersions: (spaceId: string, pageId: string) =>
    invoke<PageVersion[]>("list_versions_cmd", { spaceId, pageId }),
  rollbackVersion: (spaceId: string, pageId: string, versionId: string) =>
    invoke<void>("rollback_version_cmd", { spaceId, pageId, versionId }),
  importNoteFiles: (spaceId: string, sectionId: string, paths: string[]) =>
    invoke<BatchResult<ImportedPage>>("import_note_files_cmd", { spaceId, sectionId, paths }),
  requestPageExportConfirmation: (spaceId: string, pageId: string, format: NoteExportFormat, directory: string, filename: string) =>
    invoke<string>("request_page_export_confirmation_cmd", { spaceId, pageId, format, directory, filename }),
  requestSectionExportConfirmation: (spaceId: string, sectionId: string, format: NoteExportFormat, directory: string, name: string) =>
    invoke<string>("request_section_export_confirmation_cmd", { spaceId, sectionId, format, directory, name }),
  exportPage: (spaceId: string, pageId: string, format: NoteExportFormat, directory: string, filename: string, overwrite: boolean, confirmationToken?: string) =>
    invoke<void>("export_page_cmd", { spaceId, pageId, format, directory, filename, overwrite, confirmationToken }),
  exportSection: (spaceId: string, sectionId: string, format: NoteExportFormat, directory: string, name: string, overwrite: boolean, confirmationToken?: string) =>
    invoke<void>("export_section_cmd", { spaceId, sectionId, format, directory, name, overwrite, confirmationToken }),

  // 最近使用
  recordPageOpen: (spaceId: string, pageId: string) =>
    invoke<void>("record_page_open_cmd", { spaceId, pageId }),
  listRecent: (limit: number) => invoke<RecentEntry[]>("list_recent_cmd", { limit }),

  // 附件
  saveAttachment: (spaceId: string, sectionId: string, entityId: string, fileName: string, mime: string | null, dataBase64: string) =>
    invoke<Attachment>("save_attachment_cmd", { spaceId, sectionId, entityId, fileName, mime, dataBase64 }),
  openAttachment: (spaceId: string, attachmentId: string) =>
    invoke<AttachmentData>("open_attachment_cmd", { spaceId, attachmentId }),
  deleteAttachment: (spaceId: string, attachmentId: string) =>
    invoke<void>("delete_attachment_cmd", { spaceId, attachmentId }),
  listAttachments: (spaceId: string, entityId: string) =>
    invoke<Attachment[]>("list_attachments_cmd", { spaceId, entityId }),

  // 搜索
  searchNotes: (spaceId: string, query: string, notebookId: string | null, sectionId: string | null, limit: number) =>
    invoke<SearchResult[]>("search_notes_cmd", { spaceId, query, notebookId, sectionId, limit }),

  // 加密分区
  setSectionPassword: (spaceId: string, sectionId: string, password: string, confirmIrrecoverable: boolean) =>
    invoke<void>("set_section_password_cmd", { spaceId, sectionId, password, confirmIrrecoverable }),
  unlockSection: (spaceId: string, sectionId: string, password: string) =>
    invoke<void>("unlock_section_cmd", { spaceId, sectionId, password }),
  lockSection: (spaceId: string, sectionId: string) =>
    invoke<void>("lock_section_cmd", { spaceId, sectionId }),
  lockAllSections: () => invoke<void>("lock_all_sections_cmd"),
  changeSectionPassword: (spaceId: string, sectionId: string, oldPassword: string, newPassword: string) =>
    invoke<void>("change_section_password_cmd", { spaceId, sectionId, oldPassword, newPassword }),
  removeSectionPassword: (spaceId: string, sectionId: string, password: string) =>
    invoke<void>("remove_section_password_cmd", { spaceId, sectionId, password }),
  activityHeartbeat: () => invoke<string[]>("activity_heartbeat_cmd"),
  getUnlockedSections: () => invoke<string[]>("get_unlocked_sections_cmd"),
  generatePassword: (length?: number) => invoke<string>("generate_password_cmd", { length }),
};

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary);
}

export function base64ToBytes(base64: string): Uint8Array<ArrayBuffer> {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}
