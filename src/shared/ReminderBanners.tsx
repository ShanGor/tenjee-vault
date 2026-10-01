import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { ui } from "../i18n/ui";

type Notice = { title: string; body: string; kind: string; count: number };

export function ReminderBanners() {
  const [notices, setNotices] = useState<Notice[]>([]);

  useEffect(() => {
    void invoke<Notice[]>("drain_in_app_reminders")
      .then((pending) => setNotices((current) => [...current, ...pending]))
      .catch(() => undefined);
    const unlisten = listen<Notice>("reminder-in-app", (event) => {
      setNotices((current) => [...current, event.payload]);
    });
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, []);

  if (notices.length === 0) return null;
  return (
    <div className="fixed right-4 top-4 z-50 flex w-80 flex-col gap-2" aria-live="polite">
      {notices.map((notice, index) => (
        <button
          className="rounded-lg border border-amber-300 bg-amber-50 p-3 text-left shadow-lg"
          key={`${notice.title}-${notice.body}-${index}`}
          onClick={() => setNotices((current) => current.filter((_, item) => item !== index))}
          type="button"
        >
          <strong className="block text-sm text-amber-950">{ui(notice.kind === "task" ? "任务提醒" : notice.kind === "summary" ? "Tenjee Vault 提醒" : "日程提醒")}</strong>
          <span className="text-sm text-amber-900">{notice.kind === "summary" ? ui("错过的提醒：{p0}",{p0:notice.count}) : notice.body}</span>
        </button>
      ))}
    </div>
  );
}
