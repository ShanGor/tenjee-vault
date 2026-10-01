import { ui } from "../i18n/ui";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

type Status = { text: string; failure: boolean };

/** Surface scheduler outcomes even when Settings is not the active screen. */
export function BackupStatusBanners() {
  const [statuses, setStatuses] = useState<Status[]>([]);

  useEffect(() => {
    const unlisten = listen<string>("backup-status", ({ payload }) => {
      setStatuses((current) => [...current, { text: payload, failure: /failed/i.test(payload) }]);
    });
    return () => { void unlisten.then((dispose) => dispose()); };
  }, []);

  if (statuses.length === 0) return null;
  return <div className="fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2" aria-live="polite">
    {statuses.map((status, index) => <button type="button" key={`${status.text}-${index}`} onClick={() => setStatuses((current) => current.filter((_, currentIndex) => currentIndex !== index))} className={`rounded-lg border p-3 text-left text-sm shadow-lg ${status.failure ? "border-red-300 bg-red-50 text-red-900 dark:bg-red-950 dark:text-red-100" : "border-green-300 bg-green-50 text-green-900 dark:bg-green-950 dark:text-green-100"}`}>
      <strong className="block">{ui("自动备份")}</strong>{ui(status.failure ? "自动备份失败，请检查设置中的目录和可用空间。" : "自动备份完成。")}
    </button>)}
  </div>;
}
