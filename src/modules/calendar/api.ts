import { invoke } from "@tauri-apps/api/core";

export interface EventRecord {
  id: string; title: string; description: string | null; location: string | null;
  start_at: string; end_at: string; all_day: boolean; timezone: string | null;
  recurrence_rule: string | null; lunar_recurrence: string | null; color: string | null;
  linked_page_ref: string | null; linked_task_id: string | null;
}
export interface EventInstance extends EventRecord { original_start_at: string; start_at: string; end_at: string }
export interface DayInfo { date: string; lunar_day: string; lunar_month: number | null; lunar_date: number | null; lunar_leap: boolean; term: string | null; festival: string | null }
export interface SearchHit { event_id: string; title: string; snippet: string }
export interface LinkState { page: { reference: string; exists: boolean } | null; task: { reference: string; exists: boolean } | null }
export interface Settings { lunar_overlay_enabled: boolean; festivals_enabled: boolean; solar_terms_enabled: boolean }
export interface CalendarImportResult { created: number; updated: number; copied: number; warnings: string[] }
export type NewEvent = {
  title: string; description?: string | null; location?: string | null; start_at: string; end_at: string;
  all_day: boolean; timezone?: string | null; recurrence_rule?: string | null; lunar_recurrence?: string | null;
  color?: string | null; linked_page_ref?: string | null; linked_task_id?: string | null; reminders?: number[];
};

export const calendarApi = {
  instances: (rangeStart: string, rangeEnd: string) => invoke<EventInstance[]>("calendar_instances", { rangeStart, rangeEnd }),
  create: (input: NewEvent) => invoke<EventRecord>("create_event_cmd", { input }),
  update: (id: string, patch: Record<string, unknown>) => invoke<EventRecord>("update_event_cmd", { id, patch }),
  remove: (id: string) => invoke<void>("delete_event_cmd", { id }),
  moveInstance: (eventId: string, originalStartAt: string, newStartAt: string, newEndAt: string) => invoke<void>("move_event_instance", { eventId, originalStartAt, newStartAt, newEndAt }),
  cancelInstance: (eventId: string, originalStartAt: string) => invoke<void>("cancel_event_instance", { eventId, originalStartAt }),
  reminders: (eventId: string) => invoke<{ minutes_before: number }[]>("list_event_reminders", { eventId }),
  setReminders: (eventId: string, minutes: number[]) => invoke<void>("set_event_reminders", { eventId, minutes }),
  links: (eventId: string) => invoke<LinkState>("event_link_state", { eventId }),
  overlay: (rangeStart: string, rangeEnd: string, festivalsEnabled: boolean, solarTermsEnabled: boolean) => invoke<DayInfo[]>("lunar_overlay_cmd", { rangeStart, rangeEnd, festivalsEnabled, solarTermsEnabled }),
  search: (query: string) => invoke<SearchHit[]>("search_events_cmd", { query, limit: 50 }),
  settings: () => invoke<Settings>("get_settings"),
  setSetting: (key: string, value: boolean) => invoke<Settings>("set_setting_cmd", { key, value: String(value) }),
  importIcal: (path: string, conflict: "update" | "copy") => invoke<CalendarImportResult>("import_calendar_ical_cmd", { path, conflict }),
  exportIcal: (ids: string[] | null, rangeStart: string | null, rangeEnd: string | null, directory: string, filename: string, overwrite = false) =>
    invoke<void>("export_calendar_ical_cmd", { ids, rangeStart, rangeEnd, directory, filename, overwrite }),
};
