import { ui, getUILocale } from "../i18n/ui";
import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, type AppSettings, type BackupSummary, type RestoreDiagnostic } from "../modules/notes/api";
import { usePreferences } from "../i18n";
import { ACTIONS, type ActionId } from "../shared/actions";
import { defaultShortcuts, shortcutConflicts, type ShortcutMap } from "../shared/shortcuts";
import { useNotesStore } from "../modules/notes/store";

type Notice = { tone: "success" | "error" | "info"; text: string };

function splitPath(path: string): { directory: string; filename: string } | null {
  const index = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (index < 1 || !path.slice(index + 1)) return null;
  return { directory: path.slice(0, index), filename: path.slice(index + 1) };
}

function readableBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

export default function SettingsPage() {
  const preferences = usePreferences();
  const { formatError, t } = preferences;
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState<"backup" | "restore" | null>(null);
  const [summary, setSummary] = useState<BackupSummary | null>(null);
  const [diagnostic, setDiagnostic] = useState<RestoreDiagnostic | null>(null);
  useEffect(() => { setNotice(null); }, [preferences.locale]);

  const refresh = async () => {
    const [nextSettings, nextDiagnostic] = await Promise.all([api.getSettings(), api.restoreDiagnostic()]);
    setSettings(nextSettings);
    setDiagnostic(nextDiagnostic);
  };
  useEffect(() => { void refresh().catch((error) => setNotice({ tone: "error", text: formatError(error) })); }, [formatError]);

  async function set(key: string, value: string) {
    try {
      const saved = await api.setSetting(key, value);
      setSettings(saved);
      useNotesStore.setState({ settings: saved });
      setNotice({ tone: "success", text: t("settings.saved") });
    } catch (error) {
      setNotice({ tone: "error", text: formatError(error) });
    }
  }

  async function reset(group: string) {
    try {
      const saved = await api.resetSettings(group);
      setSettings(saved); useNotesStore.setState({ settings: saved });
      window.dispatchEvent(new CustomEvent("preferences-changed", { detail: saved }));
      window.dispatchEvent(new CustomEvent("app-shortcuts-changed", { detail: { ...defaultShortcuts(), ...JSON.parse(saved.app_shortcuts) } }));
      setNotice({ tone: "success", text: t("settings.saved") });
    } catch (error) { setNotice({ tone: "error", text: formatError(error) }); }
  }

  async function saveShortcut(id: ActionId, value: string) {
    if (!settings) return;
    try {
      const current: ShortcutMap = { ...defaultShortcuts(), ...JSON.parse(settings.app_shortcuts) };
      const next = { ...current, [id]: value };
      const conflict = shortcutConflicts(next);
      if (conflict) throw new Error(ui("快捷键冲突：{p0}", { p0: String(conflict) }));
      const saved = await api.setSetting("app_shortcuts", JSON.stringify(next));
      setSettings(saved);
      window.dispatchEvent(new CustomEvent<ShortcutMap>("app-shortcuts-changed", { detail: next }));
      setNotice({ tone: "success", text: t("settings.shortcuts-saved") });
    } catch (error) { setNotice({ tone: "error", text: formatError(error) }); }
  }

  async function chooseBackupDirectory() {
    const picked = await open({ directory: true, multiple: false, title: ui("选择自动备份目录") });
    if (typeof picked === "string") await set("auto_backup_directory", picked);
  }

  async function createManualBackup() {
    const selected = await save({
      title: ui("创建备份"),
      defaultPath: `tenjee-vault-${new Date().toISOString().replace(/:/g, "-").slice(0, 19)}.tvault`,
      filters: [{ name: ui("Tenjee Vault 备份"), extensions: ["tvault"] }],
    });
    if (!selected) return;
    const target = splitPath(selected);
    if (!target) return setNotice({ tone: "error", text: ui("无法读取所选备份路径。") });
    setBusy("backup");
    try {
      let result: BackupSummary;
      try {
        result = await api.createBackup(target.directory, target.filename, false);
      } catch (error) {
        if (!String(error).includes("目标已存在") || !window.confirm(ui("目标已存在。确定覆盖它吗？"))) throw error;
        result = await api.createBackup(target.directory, target.filename, true);
      }
      const verified = await api.verifyBackup(target.directory, target.filename);
      setSummary(result);
      setNotice({ tone: "success", text: ui("备份已验证：{p0} 个文件，创建于 {p1}。", { p0: String(verified.files), p1: String(new Date(verified.created_at).toLocaleString(getUILocale())) }) });
    } catch (error) {
      setNotice({ tone: "error", text: ui("备份未完成：{p0}", { p0: String(formatError(error)) }) });
    } finally {
      setBusy(null);
    }
  }

  async function prepareRestore() {
    const selected = await open({ multiple: false, title: ui("选择要恢复的备份"), filters: [{ name: ui("Tenjee Vault 备份"), extensions: ["tvault"] }] });
    if (typeof selected !== "string") return;
    const target = splitPath(selected);
    if (!target) return setNotice({ tone: "error", text: ui("无法读取所选恢复路径。") });
    setBusy("restore");
    try {
      const preview = await api.verifyBackup(target.directory, target.filename);
      if (!window.confirm(ui("已通过预检：{p0} 个文件，应用版本 {p1}，创建于 {p2}。\n\n恢复将替换当前全部本地数据，并在下次启动时执行。当前数据会保留为恢复前副本。继续吗？", { p0: String(preview.files), p1: String(preview.app_version), p2: String(new Date(preview.created_at).toLocaleString(getUILocale())) }))) return;
      await api.prepareRestore(target.directory, target.filename);
      setNotice({ tone: "success", text: ui("备份已预检并准备恢复。加密会话已锁定；请安全退出并重新启动应用以完成恢复。") });
    } catch (error) {
      setNotice({ tone: "error", text: ui("恢复预检失败：{p0}", { p0: String(formatError(error)) }) });
    } finally {
      setBusy(null);
    }
  }

  async function clearRecoveryCopies() {
    if (!window.confirm(ui("删除所有受管理的恢复前副本？此操作无法撤销。"))) return;
    try {
      const result = await api.clearPreRestoreCopies();
      setNotice({ tone: "success", text: result.removed ? ui("已删除 {p0} 个恢复前副本。", { p0: String(result.removed) }) : ui("没有可清理的恢复前副本。") });
    } catch (error) { setNotice({ tone: "error", text: ui("无法清理恢复前副本：{p0}", { p0: String(formatError(error)) }) }); }
  }

  if (!settings) return <main className="p-6">{t("settings.loading")}</main>;
  return <main className="settings-page mx-auto max-w-3xl space-y-6 p-6" aria-labelledby="settings-title">
    <div><h1 id="settings-title" className="text-2xl font-semibold">{t("settings.title")}</h1><p className="mt-1 text-sm text-neutral-500">{t("settings.local-only")}</p></div>
    <section className="rounded-lg border p-4"><h2>{t("settings.security")}</h2><div className="mt-3 grid gap-3">
      <label>{t("settings.auto-lock")} <input className="rounded border p-1" type="number" min="1" value={settings.section_auto_lock_minutes} onChange={(event) => void set("section_auto_lock_minutes",event.target.value)} /></label>
      <label>{t("settings.clipboard")} <input className="rounded border p-1" type="number" min="0" value={settings.clipboard_auto_clear_seconds} onChange={(event) => void set("clipboard_auto_clear_seconds",event.target.value)} /></label>
      <label><input type="checkbox" checked={settings.encrypted_section_show_titles} onChange={(event) => void set("encrypted_section_show_titles",String(event.target.checked))} /> {t("settings.show-titles")}</label>
    </div></section>
    <section className="rounded-lg border p-4"><h2>{t("settings.calendar")}</h2><div className="mt-3 flex flex-wrap gap-3">
      {([ ["lunar_overlay_enabled","settings.lunar"], ["festivals_enabled","settings.festivals"], ["solar_terms_enabled","settings.solar-terms"] ] as const).map(([key,label]) => <label key={key}><input type="checkbox" checked={settings[key]} onChange={(event) => void set(key,String(event.target.checked))} /> {t(label)}</label>)}
    </div></section>
    <section className="rounded-lg border p-4"><h2>{t("settings.reset-groups")}</h2><div className="mt-3 flex flex-wrap gap-3">
      {([ ["security","settings.security"], ["calendar","settings.calendar"], ["appearance","settings.appearance-group"], ["desktop","settings.desktop-group"], ["shortcuts","settings.shortcuts-group"], ["backup","settings.backup-group"] ] as const).map(([group,label]) => <button className="rounded border p-2" key={group} onClick={() => void reset(group)}>{t(label)}: {t("settings.reset")}</button>)}
    </div></section>
    {notice && <div role="status" className={`rounded border p-3 text-sm ${notice.tone === "error" ? "border-red-300 bg-red-50 text-red-800 dark:bg-red-950 dark:text-red-200" : notice.tone === "success" ? "border-green-300 bg-green-50 text-green-800 dark:bg-green-950 dark:text-green-200" : "border-blue-300 bg-blue-50 text-blue-800"}`}>{notice.text}</div>}
    <section className="rounded-lg border p-4"><h2 className="text-lg font-medium">{t("settings.manual-backup")}</h2><p className="mt-1 text-sm text-neutral-500">{t("settings.manual-backup-description")}</p><button className="mt-3 rounded bg-blue-600 px-3 py-2 text-white disabled:opacity-50" disabled={busy !== null} onClick={() => void createManualBackup()}>{busy === "backup" ? t("settings.backup-creating") : t("settings.backup-now")}</button>{summary && <p className="mt-3 text-sm"><code>{summary.path}</code>（{summary.files}{ui("个文件，")}{readableBytes(summary.total_bytes)}）。</p>}</section>
    <section className="rounded-lg border p-4"><h2 className="text-lg font-medium">{t("settings.auto-backup")}</h2><div className="mt-3 grid gap-3 sm:grid-cols-2"><label className="flex items-center gap-2"><input type="checkbox" checked={settings.auto_backup_enabled} onChange={(event) => void set("auto_backup_enabled", String(event.target.checked))} />{t("settings.enable-auto-backup")}</label><label>{t("settings.schedule")}<select className="ml-2 rounded border p-1" value={settings.auto_backup_schedule} onChange={(event) => void set("auto_backup_schedule", event.target.value)}><option value="daily">{t("settings.daily")}</option><option value="weekly">{t("settings.weekly")}</option></select></label><label className="sm:col-span-2">{t("settings.directory")}<div className="mt-1 flex gap-2"><input className="min-w-0 flex-1 rounded border p-2" readOnly value={settings.auto_backup_directory || t("settings.not-selected")} /><button className="rounded border px-3" onClick={() => void chooseBackupDirectory()}>{t("settings.choose")}</button></div></label><label>{t("settings.retention")}<input className="ml-2 w-20 rounded border p-1" min="1" max="365" type="number" value={settings.auto_backup_retention_count} onChange={(event) => void set("auto_backup_retention_count", event.target.value)} /></label><p className="self-end text-sm text-neutral-500">{t("settings.last-success", { value: settings.auto_backup_last_success_at ? new Date(settings.auto_backup_last_success_at).toLocaleString(getUILocale()) : t("settings.never") })}</p></div></section>
    <section className="rounded-lg border p-4"><h2 className="text-lg font-medium">{t("settings.desktop")}</h2><label className="mt-3 flex items-center gap-2"><input type="checkbox" checked={settings.close_to_tray} onChange={(event) => void set("close_to_tray", String(event.target.checked))} />{t("settings.close-to-tray")}</label><p className="mt-1 text-sm text-neutral-500">{t("settings.tray-note")}</p></section>
    <section className="rounded-lg border p-4"><h2 className="text-lg font-medium">{t("settings.shortcuts")}</h2><p className="mt-1 text-sm text-neutral-500">{t("settings.shortcuts-note")}</p><div className="mt-3 grid gap-3 sm:grid-cols-2">{ACTIONS.map((action) => { let current: ShortcutMap = defaultShortcuts(); try { current = { ...current, ...JSON.parse(settings.app_shortcuts) }; } catch { /* use defaults */ } return <label key={action.id} className="text-sm"><span>{t(action.label)}</span><input className="mt-1 block w-full rounded border p-2" key={settings.app_shortcuts} defaultValue={current[action.id] ?? ""} placeholder="Mod+Shift+K" onBlur={(event) => void saveShortcut(action.id, event.target.value)} /></label>; })}</div></section>
    <section className="rounded-lg border p-4"><h2 className="text-lg font-medium">{t("settings.appearance")}</h2><div className="mt-3 flex flex-wrap gap-4"><label>{t("settings.language")}<select className="ml-2 rounded border p-1" value={preferences.locale} onChange={(event) => void preferences.setLocale(event.target.value as typeof preferences.locale).catch((error) => setNotice({ tone: "error", text: formatError(error) }))}><option value="system">{t("locale.system")}</option><option value="zh-CN">{t("locale.zh-CN")}</option><option value="en">{t("locale.en")}</option></select></label><label>{t("settings.theme")}<select className="ml-2 rounded border p-1" value={preferences.theme} onChange={(event) => void preferences.setTheme(event.target.value as typeof preferences.theme).catch((error) => setNotice({ tone: "error", text: formatError(error) }))}><option value="system">{t("theme.system")}</option><option value="light">{t("theme.light")}</option><option value="dark">{t("theme.dark")}</option></select></label></div></section>
    <section className="rounded-lg border border-red-300 p-4"><h2 className="text-lg font-medium text-red-700 dark:text-red-300">{t("settings.restore")}</h2><p className="mt-1 text-sm text-neutral-500">{t("settings.restore-description")}</p><button className="mt-3 rounded border border-red-500 px-3 py-2 text-red-700 disabled:opacity-50 dark:text-red-300" disabled={busy !== null} onClick={() => void prepareRestore()}>{busy === "restore" ? t("settings.restore-checking") : t("settings.restore-choose")}</button><button className="ml-2 rounded border px-3 py-2" onClick={() => void clearRecoveryCopies()}>{t("settings.restore-clear")}</button>{diagnostic && <div className="mt-3 rounded border border-amber-300 bg-amber-50 p-3 text-sm text-amber-900 dark:bg-amber-950 dark:text-amber-100"><div className="mt-1">{diagnostic.message}</div><code className="mt-1 block break-all text-xs">{diagnostic.failed_candidate}</code></div>}</section>
  </main>;
}
