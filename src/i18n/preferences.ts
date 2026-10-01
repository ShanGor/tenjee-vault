import { createContext, useContext } from "react";

export const zhCN = {
  "app.name": "天机匣",
  "app.mark": "天",
  "settings.security": "安全", "settings.calendar": "日历", "settings.reset": "恢复此组默认值", "settings.auto-lock": "闲置锁定（分钟）", "settings.clipboard": "剪贴板清除（秒，0 为关闭）", "settings.show-titles": "显示锁定页面标题", "settings.lunar": "显示农历", "settings.festivals": "显示节日", "settings.solar-terms": "显示节气", "settings.reset-groups": "恢复分组默认值", "settings.backup-group": "备份", "settings.appearance-group": "外观", "settings.shortcuts-group": "快捷键", "settings.desktop-group": "桌面",
  "links.title": "待办关联任务", "links.create": "创建关联任务", "links.unlink": "解除关联", "links.open-task": "打开任务", "links.open-note": "打开来源笔记", "links.missing": "关联已失效", "links.locked": "来源页面已锁定，请先解锁",
  "templates.blank": "空白页面", "templates.meeting": "会议记录", "templates.journal": "日记", "templates.save": "保存为模板", "templates.name": "模板名称",
  "templates.global-plaintext": "保存为全局明文模板", "templates.section-only": "仅在此受保护页面解锁后可用。", "templates.plaintext-warning": "模板及其附件将以明文存储，可在其他页面使用。确定继续？",
  "templates.create": "从模板新建页面", "templates.choose": "模板", "templates.untitled": "未命名页面", "templates.section": "受保护页面", "templates.global": "全局",
  "templates.manage": "管理自定义模板", "templates.delete": "删除模板", "templates.export": "导出为全局模板", "templates.delete-confirm": "删除此模板？已创建的页面不受影响。",
  "templates.password-rule": "移除页面密码前，必须先导出并删除或直接删除私有模板。导出不会自动删除加密副本。",
  "tags.title": "标签", "tags.new": "新标签名称", "tags.create": "创建并添加", "tags.browse": "查看所有标签", "tags.remove-missing": "移除失效标签",
  "tags.type": "类型", "tags.all": "全部类型", "tags.include-archived": "包含归档任务", "tags.rename": "重命名", "tags.delete": "删除标签",
  "tags.confirm-delete": "删除标签及其关联？页面、任务和事件会保留。", "tags.partial": "部分空间不可用，结果不完整。", "tags.archived": "已归档",
  "tags.status-todo": "待办", "tags.status-in_progress": "进行中", "tags.status-done": "完成", "tags.status-cancelled": "已取消",
  "nav.search": "搜索与命令", "nav.notes": "笔记", "nav.tasks": "任务", "nav.calendar": "日程", "nav.settings": "设置",
  "settings.title": "设置", "settings.language": "语言", "settings.theme": "主题",
  "settings.loading": "正在加载设置…", "settings.saved": "设置已保存。", "settings.shortcuts-saved": "快捷键已保存。",
  "settings.local-only": "备份、恢复与应用偏好均保存在本机。", "settings.manual-backup": "手动备份",
  "settings.manual-backup-description": "生成一个可验证的 .tvault 文件。备份不会中断当前业务连接。",
  "settings.backup-now": "立即备份…", "settings.backup-creating": "正在创建备份…", "settings.auto-backup": "自动备份",
  "settings.enable-auto-backup": "启用自动备份", "settings.schedule": "计划", "settings.daily": "每天", "settings.weekly": "每周",
  "settings.directory": "目录", "settings.not-selected": "未选择", "settings.choose": "选择…", "settings.retention": "保留份数",
  "settings.last-success": "上次成功：{value}", "settings.never": "尚无",
  "settings.shortcuts": "应用内快捷键", "settings.shortcuts-note": "使用 Mod 表示 Ctrl（Windows/Linux）或 ⌘（macOS）；留空会停用该应用内快捷键。",
  "settings.appearance": "外观与语言", "settings.restore": "恢复备份",
  "settings.restore-description": "恢复前会验证包结构、哈希和数据库完整性；当前数据会被保留为恢复前副本。",
  "settings.restore-choose": "选择备份并恢复…", "settings.restore-checking": "正在预检…", "settings.restore-clear": "清理恢复前副本",
  "command.actions": "操作", "command.label": "命令面板", "command.placeholder": "输入命令或搜索全部内容…", "command.empty": "没有匹配项。",
  "result.note": "笔记", "result.task": "任务", "result.calendar": "日程",
  "quick.note": "快速笔记", "quick.task": "快速任务", "quick.title": "标题", "quick.task-list": "任务列表", "quick.due-date": "截止日期",
  "common.cancel": "取消", "common.save": "保存", "common.saving": "保存中…", "error.title-required": "请输入标题。",
  "theme.system": "跟随系统", "theme.light": "浅色", "theme.dark": "深色",
  "locale.system": "跟随系统", "locale.zh-CN": "中文", "locale.en": "English",
  "action.open-command-palette": "打开命令面板", "action.quick-note": "快速新建笔记",
  "action.quick-task": "快速新建任务", "action.go-notes": "前往笔记",
  "action.go-tasks": "前往任务", "action.go-calendar": "前往日程",
  "action.go-settings": "打开设置", "action.lock-all": "锁定全部受保护页面",
  "error.db_integrity": "数据库完整性检查失败：{detail}", "error.migration": "数据迁移失败：{detail}",
  "error.crypto": "加密操作失败：{detail}", "error.wrong_password": "页面密码错误",
  "error.not_found": "未找到：{detail}", "error.section_locked": "页面已锁定：{section}",
  "error.validation": "输入无效：{detail}", "error.io": "文件操作失败：{detail}",
  "error.sqlite": "数据库操作失败：{detail}", "error.unknown": "发生未知错误",
} as const;
export const en: { [K in keyof typeof zhCN]: string } = {
  "app.name": "Tenjee Vault",
  "app.mark": "T",
  "settings.security": "Security", "settings.calendar": "Calendar", "settings.reset": "Reset this group", "settings.auto-lock": "Idle lock (minutes)", "settings.clipboard": "Clear clipboard (seconds; 0 disables)", "settings.show-titles": "Show locked page titles", "settings.lunar": "Show lunar dates", "settings.festivals": "Show festivals", "settings.solar-terms": "Show solar terms", "settings.reset-groups": "Restore group defaults", "settings.backup-group": "Backup", "settings.appearance-group": "Appearance", "settings.shortcuts-group": "Shortcuts", "settings.desktop-group": "Desktop",
  "links.title": "Linked todo tasks", "links.create": "Create linked task", "links.unlink": "Unlink", "links.open-task": "Open task", "links.open-note": "Open source note", "links.missing": "Link no longer available", "links.locked": "Unlock the source page first",
  "templates.blank": "Blank page", "templates.meeting": "Meeting notes", "templates.journal": "Journal", "templates.save": "Save as template", "templates.name": "Template name",
  "templates.global-plaintext": "Save as a global plaintext template", "templates.section-only": "Available only while this protected page is unlocked.", "templates.plaintext-warning": "The template and its attachments will be stored in plaintext and available to other pages. Continue?",
  "templates.create": "New page from template", "templates.choose": "Template", "templates.untitled": "Untitled page", "templates.section": "Protected page", "templates.global": "Global",
  "templates.manage": "Manage custom templates", "templates.delete": "Delete template", "templates.export": "Export as global template", "templates.delete-confirm": "Delete this template? Existing pages will be kept.",
  "templates.password-rule": "Before removing a page password, export and delete its templates, or delete them directly. Exporting keeps the encrypted copy until you delete it.",
  "tags.title": "Tags", "tags.new": "New tag name", "tags.create": "Create and add", "tags.browse": "Browse tags", "tags.remove-missing": "Remove missing tag",
  "tags.type": "Type", "tags.all": "All types", "tags.include-archived": "Include archived tasks", "tags.rename": "Rename", "tags.delete": "Delete tag",
  "tags.confirm-delete": "Delete this tag and its associations? Notes, tasks and events will be kept.", "tags.partial": "Some spaces are unavailable; results are incomplete.", "tags.archived": "Archived",
  "tags.status-todo": "To do", "tags.status-in_progress": "In progress", "tags.status-done": "Done", "tags.status-cancelled": "Cancelled",
  "nav.search": "Search & commands", "nav.notes": "Notes", "nav.tasks": "Tasks", "nav.calendar": "Calendar", "nav.settings": "Settings",
  "settings.title": "Settings", "settings.language": "Language", "settings.theme": "Theme",
  "settings.loading": "Loading settings…", "settings.saved": "Settings saved.", "settings.shortcuts-saved": "Shortcuts saved.",
  "settings.local-only": "Backups, recovery, and app preferences are stored on this device.", "settings.manual-backup": "Manual backup",
  "settings.manual-backup-description": "Create a verifiable .tvault file without interrupting active connections.",
  "settings.backup-now": "Back up now…", "settings.backup-creating": "Creating backup…", "settings.auto-backup": "Automatic backup",
  "settings.enable-auto-backup": "Enable automatic backups", "settings.schedule": "Schedule", "settings.daily": "Daily", "settings.weekly": "Weekly",
  "settings.directory": "Directory", "settings.not-selected": "Not selected", "settings.choose": "Choose…", "settings.retention": "Retention",
  "settings.last-success": "Last successful backup: {value}", "settings.never": "Never",
  "settings.shortcuts": "In-app shortcuts", "settings.shortcuts-note": "Use Mod for Ctrl (Windows/Linux) or ⌘ (macOS); leave blank to disable an in-app shortcut.",
  "settings.appearance": "Appearance and language", "settings.restore": "Restore backup",
  "settings.restore-description": "Before restore, the app verifies package structure, hashes, and database integrity; current data is retained as a pre-restore copy.",
  "settings.restore-choose": "Choose backup and restore…", "settings.restore-checking": "Checking backup…", "settings.restore-clear": "Clear pre-restore copies",
  "command.actions": "Actions", "command.label": "Command palette", "command.placeholder": "Type a command or search everything…", "command.empty": "No matches.",
  "result.note": "Notes", "result.task": "Tasks", "result.calendar": "Calendar",
  "quick.note": "Quick note", "quick.task": "Quick task", "quick.title": "Title", "quick.task-list": "Task list", "quick.due-date": "Due date",
  "common.cancel": "Cancel", "common.save": "Save", "common.saving": "Saving…", "error.title-required": "Enter a title.",
  "theme.system": "System", "theme.light": "Light", "theme.dark": "Dark",
  "locale.system": "System", "locale.zh-CN": "Chinese", "locale.en": "English",
  "action.open-command-palette": "Open command palette", "action.quick-note": "Quick note",
  "action.quick-task": "Quick task", "action.go-notes": "Go to notes",
  "action.go-tasks": "Go to tasks", "action.go-calendar": "Go to calendar",
  "action.go-settings": "Open settings", "action.lock-all": "Lock all protected pages",
  "error.db_integrity": "Database integrity check failed: {detail}", "error.migration": "Data migration failed: {detail}",
  "error.crypto": "Encryption operation failed: {detail}", "error.wrong_password": "Incorrect page password",
  "error.not_found": "Not found: {detail}", "error.section_locked": "Page is locked: {section}",
  "error.validation": "Invalid input: {detail}", "error.io": "File operation failed: {detail}",
  "error.sqlite": "Database operation failed: {detail}", "error.unknown": "An unknown error occurred",
};
export type MessageKey = keyof typeof zhCN;
export type Locale = "system" | "zh-CN" | "en";
export type Theme = "system" | "light" | "dark";
export type MessageParams = Record<string, string | number>;
export type Preferences = { locale: Locale; theme: Theme; t(key: MessageKey, params?: MessageParams): string; formatError(error: unknown): string; setLocale(locale: Locale): Promise<void>; setTheme(theme: Theme): Promise<void> };
export const PreferencesContext = createContext<Preferences | null>(null);

export function format(template: string, params: MessageParams = {}) {
  return template.replace(/\{([a-z_]+)\}/g, (whole, name: string) => String(params[name] ?? whole));
}

export function resolvedLocale(locale: Locale) { return locale === "system" ? (navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en") : locale; }
export function prefersDark() { return window.matchMedia("(prefers-color-scheme: dark)").matches; }
export function cached(name: "locale" | "theme", fallback: Locale | Theme) {
  try {
    const value = localStorage.getItem(`tenjee-vault-${name}`);
    return value ?? fallback;
  } catch { return fallback; }
}

export function bootstrapDocumentPreferences() {
  const locale = cached("locale", "system") as Locale;
  const theme = cached("theme", "system") as Theme;
  document.documentElement.lang = resolvedLocale(locale);
  document.title = resolvedLocale(locale) === "zh-CN" ? "天机匣" : "Tenjee Vault";
  document.documentElement.classList.toggle("dark", theme === "dark" || (theme === "system" && prefersDark()));
}

export function usePreferences() { const value = useContext(PreferencesContext); if (!value) throw new Error("PreferencesProvider is required"); return value; }
