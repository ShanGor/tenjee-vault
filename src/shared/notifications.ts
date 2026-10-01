import { getUILocale, ui } from "../i18n/ui";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

/** Ask once at application startup. Browser-only preview failures degrade silently. */
export async function ensureNotificationPermission(): Promise<boolean> {
  try {
    if (await isPermissionGranted()) return true;
    return (await requestPermission()) === "granted";
  } catch {
    return false;
  }
}

/** Development/settings diagnostic entrypoint; never sends unless permission is granted. */
export async function sendTestNotification(): Promise<boolean> {
  if (!(await ensureNotificationPermission())) return false;
  sendNotification({
    title: getUILocale() === "zh-CN" ? "天机匣" : "Tenjee Vault",
    body: ui("通知功能已就绪"),
  });
  return true;
}
