import { invoke as coreInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { isAndroid } from "./nativeFiles";

export interface ReminderStatus { supported: boolean; permissionGranted: boolean; exact: boolean; scheduled: number; horizon: number; deferred: number }

export async function reconcileReminders(): Promise<ReminderStatus | null> {
  if (!isAndroid()) return null;
  const status = await coreInvoke<ReminderStatus>("mobile_reminders_reconcile_cmd");
  window.dispatchEvent(new CustomEvent("mobile-reminder-status", { detail: status }));
  return status;
}

export async function invoke<T>(command: string, args?: InvokeArgs): Promise<T> {
  const result = await coreInvoke<T>(command, args);
  if (isAndroid() && /^(create|update|delete|set_|move_|cancel_|archive|restore|bulk_|reorder_|import_|complete_|generate_)/.test(command)) {
    try { await reconcileReminders(); }
    catch (error) { window.dispatchEvent(new CustomEvent("mobile-reminder-error", { detail: error })); }
  }
  return result;
}
