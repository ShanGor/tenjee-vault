import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ui, uiError, getUILocale } from "../i18n/ui";
import { isAndroid } from "./nativeFiles";
import { reconcileReminders, type ReminderStatus } from "./mobileReminders";

export function ReminderSettings() {
  const [status, setStatus] = useState<ReminderStatus | null>(null);
  const [error, setError] = useState("");
  const refresh = async () => {
    try { setStatus(await reconcileReminders()); setError(""); }
    catch (reason) { setError(String(uiError(reason))); }
  };
  useEffect(() => {
    if (!isAndroid()) return;
    const update = (event: Event) => setStatus((event as CustomEvent<ReminderStatus>).detail);
    window.addEventListener("mobile-reminder-status", update);
    void refresh();
    return () => window.removeEventListener("mobile-reminder-status", update);
  }, []);
  if (!isAndroid()) return null;
  return <section className="rounded-lg border p-4"><h2>{ui("后台提醒")}</h2>
    {error && <p role="alert">{ui("提醒安排失败：{p0}", { p0: error })}</p>}
    {status && <div className="mt-3 grid gap-3 text-sm">
      {!status.permissionGranted && <p role="alert">{ui("通知权限未开启。请在系统设置中允许通知。")}</p>}
      {!status.exact && <p>{ui("系统可能延迟提醒；可在系统设置中允许精确闹钟。")}</p>}
      <p>{ui("已安排 {p0} 条提醒，安排窗口至 {p1}。请在此日期前再次打开应用以刷新。", {p0: status.scheduled, p1: new Date(status.horizon).toLocaleString(getUILocale())})}</p>
      {!!status.deferred && <p>{ui("另有 {p0} 条提醒等待下一次刷新。", {p0: status.deferred})}</p>}
    </div>}
    <div className="mt-3 flex flex-wrap gap-3"><button className="rounded border p-2" onClick={() => void refresh()}>{ui("刷新提醒")}</button><button className="rounded border p-2" onClick={() => void invoke("mobile_alarm_settings_cmd").catch(reason => setError(String(uiError(reason))))}>{ui("精确闹钟设置")}</button></div>
  </section>;
}
