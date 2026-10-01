import { flushPageSave } from "./modules/notes/pageSave";
import { Icon, type IconName } from "./shared/Icon";
import { ui } from "./i18n/ui";
import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { CalendarApp } from "./modules/calendar";
import { NotesApp } from "./modules/notes";
import { TasksApp } from "./modules/tasks";
import { ensureNotificationPermission } from "./shared/notifications";
import { ReminderBanners } from "./shared/ReminderBanners";
import { BackupStatusBanners } from "./shared/BackupStatusBanners";
import SettingsPage from "./settings/SettingsPage";
import { TagsPage } from "./shared/Tags";
import { CommandPanel } from "./shared/CommandPanel";
import { type ActionId } from "./shared/actions";
import { actionForKeyboardEvent, defaultShortcuts, type ShortcutMap } from "./shared/shortcuts";
import { api as notesApi } from "./modules/notes/api";
import { taskApi } from "./modules/tasks/api";
import { usePreferences } from "./i18n";
import { invoke } from "@tauri-apps/api/core";

export default function App() {
  const { t } = usePreferences();
  const [path, setPath] = useState(() => window.location.hash.slice(1) || "/notes");
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [capture, setCapture] = useState<"note" | "task" | null>(null);
  const [shortcuts, setShortcuts] = useState<ShortcutMap>(defaultShortcuts);

  const runAction = useCallback((id: ActionId) => {
    if (id === "open-command-palette") return setPaletteOpen(true);
    if (id === "quick-note") return setCapture("note");
    if (id === "quick-task") return setCapture("task");
    if (id === "lock-all") { void flushPageSave().then(() => notesApi.lockAllSections()); return; }
    const routes: Partial<Record<ActionId, string>> = { "go-notes": "#/notes", "go-tasks": "#/tasks", "go-calendar": "#/calendar", "go-settings": "#/settings" };
    if (routes[id]) window.location.hash = routes[id]!;
  }, []);

  useEffect(() => {
    void invoke<boolean>("release_probe_enabled").then(async (enabled) => {
      if (!enabled) { await ensureNotificationPermission(); return; }
      await Promise.all([notesApi.getSettings(),notesApi.listSpaces(),taskApi.lists()]);
      requestAnimationFrame(() => requestAnimationFrame(() => { void invoke("release_probe_ready"); }));
    }).catch(() => undefined);
  }, []);

  useEffect(() => {
    void notesApi.getSettings().then((settings) => {
      try { setShortcuts({ ...defaultShortcuts(), ...JSON.parse(settings.app_shortcuts) }); } catch { setShortcuts(defaultShortcuts()); }
    }).catch(() => undefined);
    const update = (event: Event) => setShortcuts((event as CustomEvent<ShortcutMap>).detail);
    window.addEventListener("app-shortcuts-changed", update);
    return () => window.removeEventListener("app-shortcuts-changed", update);
  }, []);

  useEffect(() => {
    const keyboard = (event: KeyboardEvent) => {
      const action = actionForKeyboardEvent(event, shortcuts);
      if (action) { event.preventDefault(); runAction(action); }
    };
    const local = (event: Event) => runAction((event as CustomEvent<ActionId>).detail);
    window.addEventListener("keydown", keyboard);
    window.addEventListener("app-action", local);
    const remote = listen<ActionId>("app-action", (event) => runAction(event.payload));
    return () => { window.removeEventListener("keydown", keyboard); window.removeEventListener("app-action", local); void remote.then((dispose) => dispose()); };
  }, [runAction, shortcuts]);

  useEffect(() => {
    const legacyNotesPath = /^(\/s\/|\/recent$|\/search$|\/trash$)/;
    if (legacyNotesPath.test(path)) {
      window.location.replace(`#\/notes${path}`);
      return;
    }
    if (!path.startsWith("/notes") && !path.startsWith("/tasks") && !path.startsWith("/calendar") && !path.startsWith("/settings") && !path.startsWith("/tags")) {
      window.location.replace("#/notes");
      return;
    }
    const onHashChange = () => setPath(window.location.hash.slice(1) || "/notes");
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, [path]);

  const module = path.startsWith("/tags") ? "tags" : path.startsWith("/tasks") ? "tasks" : path.startsWith("/calendar") ? "calendar" : path.startsWith("/settings") ? "settings" : "notes";
  const navigation: { name: string; icon: IconName; label: string }[] = [
    { name: "notes", icon: "notes", label: t("nav.notes") },
    { name: "tasks", icon: "tasks", label: t("nav.tasks") },
    { name: "calendar", icon: "calendar", label: t("nav.calendar") },
    { name: "tags", icon: "tags", label: t("tags.title") },
    { name: "settings", icon: "settings", label: t("nav.settings") },
  ];

  return (
    <div className="vault-app flex h-full flex-col">
      <ReminderBanners />
      <BackupStatusBanners />
      <nav className="app-navigation" aria-label={ui("模块导航")}>
        <a className="app-brand" href="#/notes"><span className="brand-mark">{t("app.mark")}</span><span>{t("app.name")}</span></a>
        <div className="module-links">{navigation.map((item) => <a key={item.name} className={`module-link ${module === item.name ? "is-active" : ""}`} aria-current={module === item.name ? "page" : undefined} href={`#/${item.name}`}><Icon name={item.icon} /><span>{item.label}</span></a>)}</div>
        <button className="command-trigger" onClick={() => setPaletteOpen(true)} title={t("command.label")}><Icon name="search" /><span>{t("nav.search")}</span></button>
      </nav>
      <div className={`module-content module-${module} min-h-0 flex-1`}>

        {module === "notes" && <NotesApp />}
        {module === "tasks" && <TasksApp />}
        {module === "calendar" && <CalendarApp />}
        {module === "settings" && <SettingsPage />}
        {module === "tags" && <TagsPage />}
      </div>
      <CommandPanel open={paletteOpen} onClose={() => setPaletteOpen(false)} onAction={runAction} />
      {capture && <QuickCapture kind={capture} onClose={() => setCapture(null)} />}
    </div>
  );
}

function QuickCapture({ kind, onClose }: { kind: "note" | "task"; onClose(): void }) {
  const { formatError, t } = usePreferences();
  const [title, setTitle] = useState("");
  const [lists, setLists] = useState<{ id: string; name: string }[]>([]);
  const [listId, setListId] = useState("");
  const [dueDate, setDueDate] = useState("");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (kind !== "task") return;
    void taskApi.lists().then((next) => {
      setLists(next);
      setListId((current) => current || next[0]?.id || "");
    }).catch((reason) => setError(formatError(reason)));
  }, [kind]);

  async function save() {
    if (!title.trim()) return setError(t("error.title-required"));
    setSaving(true); setError("");
    try {
      if (kind === "task") {
        const inbox = lists[0] ?? (await taskApi.lists())[0];
        const destination = listId || inbox?.id;
        if (!destination) throw new Error(ui("没有可用的任务列表。"));
        const task = await taskApi.create(destination, title.trim());
        if (dueDate) await taskApi.update(task.id, { due_date: dueDate });
        window.location.hash = `#/tasks?task=${task.id}`;
      } else {
        const spaces = await notesApi.listSpaces();
        const space = spaces[0] ?? (await notesApi.ensureDefaultSpace())[0];
        const page = await notesApi.createSpacePage(space.id, null, title.trim());
        window.location.hash = `#/notes/s/${space.id}/page/${page.id}`;
      }
      onClose();
    } catch (reason) { setError(formatError(reason)); } finally { setSaving(false); }
  }
  return <div className="fixed inset-0 z-[70] grid place-items-center bg-black/30" onMouseDown={onClose}><section className="w-[min(28rem,calc(100vw-2rem))] rounded-xl bg-white p-5 shadow-xl dark:bg-neutral-900" role="dialog" aria-modal="true" aria-labelledby="quick-capture-title" onMouseDown={(event) => event.stopPropagation()}><h2 id="quick-capture-title" className="text-lg font-semibold">{kind === "note" ? t("quick.note") : t("quick.task")}</h2><input autoFocus className="mt-4 w-full rounded border p-2" value={title} placeholder={t("quick.title")} onChange={(event) => setTitle(event.target.value)} onKeyDown={(event) => event.key === "Enter" && void save()} />{kind === "task" && <div className="mt-3 grid grid-cols-2 gap-3"><label className="text-sm">{t("quick.task-list")}<select className="mt-1 block w-full rounded border p-2" value={listId} onChange={(event) => setListId(event.target.value)}>{lists.map((list) => <option key={list.id} value={list.id}>{list.name}</option>)}</select></label><label className="text-sm">{t("quick.due-date")}<input className="mt-1 block w-full rounded border p-2" type="date" value={dueDate} onChange={(event) => setDueDate(event.target.value)} /></label></div>}{error && <p className="mt-2 text-sm text-red-600">{error}</p>}<div className="mt-4 flex justify-end gap-2"><button className="rounded border px-3 py-2" onClick={onClose}>{t("common.cancel")}</button><button className="rounded bg-blue-600 px-3 py-2 text-white disabled:opacity-50" disabled={saving} onClick={() => void save()}>{saving ? t("common.saving") : t("common.save")}</button></div></section></div>;
}
