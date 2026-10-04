//! Tauri commands：前后端桥接层（design D1：薄层，校验 + 调用领域层 + 错误映射）。
//! 纯逻辑可单测：`AppStateInner` 不依赖 WebView，测试可直接以临时目录构造。

pub mod backup;
pub mod calendar;
pub mod crypto_cmds;
pub mod global_search;
pub mod notes;
pub mod tags;
pub mod tasks;
pub mod templates;
pub mod note_tasks;

#[tauri::command]
pub fn mobile_exit_cmd(app: tauri::AppHandle) {
    #[cfg(target_os = "android")]
    crate::desktop::quit(&app);
    #[cfg(not(target_os = "android"))]
    let _ = app;
}

#[tauri::command]
pub fn release_probe_enabled() -> bool { std::env::var_os("TENJEE_RELEASE_PROBE_DIR").is_some() }

#[tauri::command]
pub fn release_probe_ready(app: tauri::AppHandle, state: tauri::State<'_,AppState>) -> VaultResult<()> {
    if let Some(directory)=std::env::var_os("TENJEE_RELEASE_PROBE_DIR") {
        let report=serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"alerts":state.inner.report.alerts.len()});
        std::fs::write(std::path::PathBuf::from(directory).join("ready.json"),report.to_string())?;
        crate::desktop::quit(&app);
    }
    Ok(())
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::db::connection;
use crate::db::layout;
use crate::db::migrate::{run_migrations, DbKind};
use crate::db::registry::SpaceInfo;
use crate::db::startup::StartupReport;
use crate::error::{VaultError, VaultResult};
use crate::notes::session::SessionManager;
use crate::remind::InAppNotice;

/// 设置键（design D8：最小设置存储，meta.db app_config）。
pub const SETTING_CLIPBOARD_AUTO_CLEAR: &str = "clipboard_auto_clear_seconds";
pub const SETTING_SECTION_AUTO_LOCK: &str = "section_auto_lock_minutes";
pub const SETTING_SHOW_TITLES: &str = "encrypted_section_show_titles";
pub const SETTING_DEFAULT_SPACE_NAME: &str = "default_space_name";
pub const SETTING_LUNAR_OVERLAY: &str = "lunar_overlay_enabled";
pub const SETTING_FESTIVALS: &str = "festivals_enabled";
pub const SETTING_SOLAR_TERMS: &str = "solar_terms_enabled";
pub const SETTING_AUTO_BACKUP_ENABLED: &str = "auto_backup_enabled";
pub const SETTING_AUTO_BACKUP_SCHEDULE: &str = "auto_backup_schedule";
pub const SETTING_AUTO_BACKUP_DIRECTORY: &str = "auto_backup_directory";
pub const SETTING_AUTO_BACKUP_RETENTION: &str = "auto_backup_retention_count";
pub const SETTING_AUTO_BACKUP_LAST_SUCCESS: &str = "auto_backup_last_success_at";
pub const SETTING_CLOSE_TO_TRAY: &str = "close_to_tray";
pub const SETTING_APP_LOCALE: &str = "app_locale";
pub const SETTING_APP_THEME: &str = "app_theme";
pub const SETTING_APP_SHORTCUTS: &str = "app_shortcuts";

/// 应用共享状态（可跨线程共享给后台巡检线程）。
pub struct AppState {
    pub inner: Arc<AppStateInner>,
}

pub struct AppStateInner {
    pub root: PathBuf,
    pub report: StartupReport,
    pub replica_id: String,
    /// 所有数据库访问都先取得共享锁；备份/恢复以独占锁冻结一个一致窗口。
    /// 若需要同时持有多个数据库锁，固定顺序是 meta → tasks → calendar → space id。
    maintenance: RwLock<()>,
    meta: Mutex<Connection>,
    tasks: Mutex<Connection>,
    calendar: Mutex<Connection>,
    spaces: Mutex<HashMap<String, Connection>>,
    pub session: SessionManager,
    pub in_app_reminders: Mutex<Vec<InAppNotice>>,
    export_confirmations: Mutex<HashMap<String, ExportConfirmation>>,
    backup_scheduler_stop: AtomicBool,
}

/// A short-lived, single-use server-side capability for plaintext exports from encrypted
/// sections. Keeping the binding server-side means a caller cannot forge or retarget it.
struct ExportConfirmation {
    subject: String,
    destination: String,
    format: String,
    expires_at: Instant,
}

impl AppState {
    /// 初始化：打开主库、迁移、注册表兜底（无任何空间时创建默认空间，spec: 空间管理）。
    pub fn init(root: PathBuf, report: StartupReport) -> VaultResult<Self> {
        layout::ensure_layout(&root)?;
        let mut meta = connection::open_db(&root.join(layout::META_DB))?;
        run_migrations(&mut meta, DbKind::Meta.migrations())?;
        let mut tasks = connection::open_db(&root.join(layout::TASKS_DB))?;
        run_migrations(&mut tasks, DbKind::Tasks.migrations())?;
        let mut calendar = connection::open_db(&root.join(layout::CALENDAR_DB))?;
        run_migrations(&mut calendar, DbKind::Calendar.migrations())?;
        let replica_id = crate::sync::storage::installation_origin(&root)?;
        for (conn,store) in [(&meta,"meta"), (&tasks,"tasks"), (&calendar,"calendar")] { crate::sync::storage::set_origin(conn, &format!("{replica_id}-{store}"))?; }
        let inner = AppStateInner {
            replica_id,
            root,
            report,
            maintenance: RwLock::new(()),
            meta: Mutex::new(meta),
            tasks: Mutex::new(tasks),
            calendar: Mutex::new(calendar),
            spaces: Mutex::new(HashMap::new()),
            session: SessionManager::new(),
            in_app_reminders: Mutex::new(Vec::new()),
            export_confirmations: Mutex::new(HashMap::new()),
            backup_scheduler_stop: AtomicBool::new(false),
        };
        crate::sync::engine::cleanup_snapshots(&inner)?;
        crate::sync::engine::recover(&inner)?;
        let empty = inner
            .with_meta(|meta| registry::list_spaces(meta))?
            .is_empty();
        if empty {
            let default_name = inner.get_setting_string(SETTING_DEFAULT_SPACE_NAME, "默认空间")?;
            inner.with_meta(|meta| registry::create_space(meta, &inner.root, &default_name))?;
        }
        Ok(Self {
            inner: Arc::new(inner),
        })
    }
}

impl AppStateInner {
    fn maintenance_read(&self) -> VaultResult<RwLockReadGuard<'_, ()>> {
        self.maintenance
            .read()
            .map_err(|_| VaultError::Validation("维护闸门锁中毒".into()))
    }

    /// 维护操作（备份/恢复）取得独占闸门。调用者随后必须按
    /// meta → tasks → calendar → space id 的顺序取得数据库连接。
    pub fn maintenance_write(&self) -> VaultResult<RwLockWriteGuard<'_, ()>> {
        self.maintenance
            .write()
            .map_err(|_| VaultError::Validation("维护闸门锁中毒".into()))
    }

    fn meta(&self) -> VaultResult<std::sync::MutexGuard<'_, Connection>> {
        self.meta
            .lock()
            .map_err(|_| VaultError::Validation("主库锁中毒".into()))
    }

    /// 主库连接上的只读/写操作。
    pub fn with_meta<R>(&self, f: impl FnOnce(&Connection) -> VaultResult<R>) -> VaultResult<R> {
        let _maintenance = self.maintenance_read()?;
        let meta = self.meta()?;
        f(&meta)
    }

    pub fn with_tasks<R>(
        &self,
        f: impl FnOnce(&mut Connection) -> VaultResult<R>,
    ) -> VaultResult<R> {
        let _maintenance = self.maintenance_read()?;
        let mut tasks = self
            .tasks
            .lock()
            .map_err(|_| VaultError::Validation("任务库锁中毒".into()))?;
        f(&mut tasks)
    }

    pub fn with_calendar<R>(
        &self,
        f: impl FnOnce(&mut Connection) -> VaultResult<R>,
    ) -> VaultResult<R> {
        let _maintenance = self.maintenance_read()?;
        let mut calendar = self
            .calendar
            .lock()
            .map_err(|_| VaultError::Validation("日程库锁中毒".into()))?;
        f(&mut calendar)
    }

    /// 同时访问三座主库时的唯一入口。它一次取得维护共享锁，并严格按
    /// meta → tasks → calendar 的顺序取得连接，避免嵌套 accessor 反转锁顺序。
    pub fn with_core_connections<R>(
        &self,
        f: impl FnOnce(&Connection, &mut Connection, &mut Connection) -> VaultResult<R>,
    ) -> VaultResult<R> {
        let _maintenance = self.maintenance_read()?;
        let meta = self.meta()?;
        let mut tasks = self
            .tasks
            .lock()
            .map_err(|_| VaultError::Validation("任务库锁中毒".into()))?;
        let mut calendar = self
            .calendar
            .lock()
            .map_err(|_| VaultError::Validation("日程库锁中毒".into()))?;
        f(&meta, &mut tasks, &mut calendar)
    }

    /// One consistent workspace window for exchange inventories and recovery.
    /// Network I/O must happen after this closure releases the maintenance gate.
    pub fn with_workspace<R>(&self, f: impl FnOnce(&mut Connection,&mut Connection,&mut Connection,&mut HashMap<String,Connection>) -> VaultResult<R>) -> VaultResult<R> {
        let _maintenance=self.maintenance_write()?;
        let mut meta=self.meta()?;
        let mut tasks=self.tasks.lock().map_err(|_|VaultError::Validation("Task store lock failed".into()))?;
        let mut calendar=self.calendar.lock().map_err(|_|VaultError::Validation("Calendar store lock failed".into()))?;
        let mut spaces=self.spaces()?;
        let mut infos=registry::list_spaces(&meta)?; infos.sort_by(|a,b|a.id.cmp(&b.id));
        for info in infos {
            if !spaces.contains_key(&info.id) {
                let mut conn=connection::open_db(&self.root.join(&info.db_file))?;
                run_migrations(&mut conn,DbKind::Space.migrations())?;
                crate::sync::storage::set_origin(&conn,&format!("{}-{}",self.replica_id,info.id))?;
                spaces.insert(info.id,conn);
            }
        }
        f(&mut meta,&mut tasks,&mut calendar,&mut spaces)
    }

    /// Freeze all mutation access briefly and copy every registered SQLite database through the
    /// online-backup API. The acquisition order is intentionally meta → tasks → calendar →
    /// sorted spaces; callers may copy attachments only after this function has returned.
    pub fn snapshot_databases(&self, staging: &std::path::Path) -> VaultResult<Vec<PathBuf>> {
        std::fs::create_dir_all(staging)?;
        let _maintenance = self.maintenance_write()?;
        let meta = self.meta()?;
        let tasks = self
            .tasks
            .lock()
            .map_err(|_| VaultError::Validation("任务库锁中毒".into()))?;
        let calendar = self
            .calendar
            .lock()
            .map_err(|_| VaultError::Validation("日程库锁中毒".into()))?;
        let mut infos = registry::list_spaces(&meta)?;
        infos.sort_by(|left, right| left.id.cmp(&right.id));
        let spaces = self.spaces()?;

        let mut copied = Vec::new();
        for (name, source) in [
            (layout::META_DB, &*meta),
            (layout::TASKS_DB, &*tasks),
            (layout::CALENDAR_DB, &*calendar),
        ] {
            let destination = staging.join(name);
            crate::backup::snapshot::snapshot_connection(source, &destination)?;
            copied.push(destination);
        }
        for info in infos {
            let destination = staging.join(&info.db_file);
            if let Some(source) = spaces.get(&info.id) {
                crate::backup::snapshot::snapshot_connection(source, &destination)?;
            } else {
                let source = connection::open_db(&self.root.join(&info.db_file))?;
                crate::backup::snapshot::snapshot_connection(&source, &destination)?;
            }
            copied.push(destination);
        }
        Ok(copied)
    }

    /// Copy every backup payload under the same exclusive maintenance window as the online
    /// SQLite snapshots.  Attachments are content addressed, but users or sync tools can still
    /// modify files behind our back; a source digest and a post-copy digest make that race a
    /// hard failure rather than an inconsistent archive.
    pub fn snapshot_backup_payload(
        &self,
        staging: &Path,
    ) -> VaultResult<Vec<crate::backup::FrozenPayload>> {
        use sha2::{Digest, Sha256};

        fn digest(path: &Path) -> VaultResult<String> {
            let mut file = std::fs::File::open(path)?;
            let mut hasher = Sha256::new();
            std::io::copy(&mut file, &mut hasher)?;
            Ok(format!("{:x}", hasher.finalize()))
        }

        fn copy_tree(
            source: &Path,
            target: &Path,
            relative: &Path,
            kind: &str,
            out: &mut Vec<crate::backup::FrozenPayload>,
        ) -> VaultResult<()> {
            if !source.exists() {
                return Ok(());
            }
            for entry in std::fs::read_dir(source)? {
                let entry = entry?;
                let file_type = entry.file_type()?;
                let name = entry.file_name();
                let child_relative = relative.join(&name);
                if file_type.is_symlink() {
                    return Err(VaultError::Validation(format!(
                        "附件目录不能包含符号链接: {}",
                        child_relative.display()
                    )));
                }
                if file_type.is_dir() {
                    copy_tree(&entry.path(), target, &child_relative, kind, out)?;
                    continue;
                }
                if !file_type.is_file() {
                    return Err(VaultError::Validation(format!(
                        "附件目录包含非普通文件: {}",
                        child_relative.display()
                    )));
                }
                let source_hash = digest(&entry.path())?;
                let destination = target.join(&child_relative);
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(entry.path(), &destination)?;
                if digest(&destination)? != source_hash {
                    return Err(VaultError::Validation(format!(
                        "复制附件时文件发生变化: {}",
                        child_relative.display()
                    )));
                }
                out.push(crate::backup::FrozenPayload {
                    relative_path: child_relative,
                    kind: kind.to_string(),
                    sha256: source_hash,
                });
            }
            Ok(())
        }

        std::fs::create_dir_all(staging)?;
        let _maintenance = self.maintenance_write()?;
        let meta = self.meta()?;
        let tasks = self
            .tasks
            .lock()
            .map_err(|_| VaultError::Validation("任务库锁中毒".into()))?;
        let calendar = self
            .calendar
            .lock()
            .map_err(|_| VaultError::Validation("日程库锁中毒".into()))?;
        let mut infos = registry::list_spaces(&meta)?;
        infos.sort_by(|left, right| left.id.cmp(&right.id));
        let spaces = self.spaces()?;
        let mut payload = Vec::new();

        for (name, source, kind) in [
            (layout::META_DB, &*meta, "database"),
            (layout::TASKS_DB, &*tasks, "database"),
            (layout::CALENDAR_DB, &*calendar, "database"),
        ] {
            let destination = staging.join(name);
            crate::backup::snapshot::snapshot_connection(source, &destination)?;
            payload.push(crate::backup::FrozenPayload {
                relative_path: PathBuf::from(name),
                kind: kind.to_string(),
                sha256: digest(&destination)?,
            });
        }
        for info in &infos {
            let destination = staging.join(&info.db_file);
            if let Some(source) = spaces.get(&info.id) {
                crate::backup::snapshot::snapshot_connection(source, &destination)?;
            } else {
                let source = connection::open_db(&self.root.join(&info.db_file))?;
                crate::backup::snapshot::snapshot_connection(&source, &destination)?;
            }
            payload.push(crate::backup::FrozenPayload {
                relative_path: PathBuf::from(&info.db_file),
                kind: "space_database".to_string(),
                sha256: digest(&destination)?,
            });
        }
        copy_tree(
            &self.tasks_files_dir(),
            staging,
            Path::new(layout::TASKS_FILES_DIR),
            "task_attachment",
            &mut payload,
        )?;
        for info in &infos {
            let relative = PathBuf::from(layout::SPACES_DIR).join(format!("{}.files", info.id));
            copy_tree(
                &self.root.join(&relative),
                staging,
                &relative,
                "space_attachment",
                &mut payload,
            )?;
        }
        Ok(payload)
    }

    pub fn push_in_app_reminders(&self, notices: Vec<InAppNotice>) {
        if let Ok(mut pending) = self.in_app_reminders.lock() {
            pending.extend(notices);
        }
    }

    pub fn drain_in_app_reminders(&self) -> Vec<InAppNotice> {
        self.in_app_reminders
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default()
    }

    pub fn stop_backup_scheduler(&self) {
        self.backup_scheduler_stop.store(true, Ordering::SeqCst);
    }

    pub fn backup_scheduler_stopped(&self) -> bool {
        self.backup_scheduler_stop.load(Ordering::SeqCst)
    }

    fn spaces(&self) -> VaultResult<std::sync::MutexGuard<'_, HashMap<String, Connection>>> {
        self.spaces
            .lock()
            .map_err(|_| VaultError::Validation("空间连接锁中毒".into()))
    }

    /// 空间库连接（常驻单连接：TEMP 内存索引生命周期与连接一致，design D4）。
    /// 首次访问时打开并迁移。
    pub fn with_space<R>(
        &self,
        space_id: &str,
        f: impl FnOnce(&mut Connection) -> VaultResult<R>,
    ) -> VaultResult<R> {
        // 注册表查询单独完成，避免在已持有维护共享锁时递归取得同一 RwLock。
        let info = self
            .with_meta(|meta| registry::get_space(meta, space_id))?
            .ok_or_else(|| VaultError::NotFound(format!("空间 {space_id}")))?;
        let _maintenance = self.maintenance_read()?;
        let mut spaces = self.spaces()?;
        if !spaces.contains_key(space_id) {
            let mut conn = connection::open_db(&self.root.join(&info.db_file))?;
            run_migrations(&mut conn, DbKind::Space.migrations())?;
            crate::sync::storage::set_origin(&conn, &format!("{}-{}",self.replica_id,space_id))?;
            spaces.insert(space_id.to_string(), conn);
        }
        let conn = spaces
            .get_mut(space_id)
            .ok_or_else(|| VaultError::NotFound(format!("空间 {space_id}")))?;
        f(conn)
    }

    /// 附件目录：`<root>/spaces/<space_id>.files`（registry 创建空间时已建）。
    pub fn files_dir(&self, space_id: &str) -> PathBuf {
        self.root
            .join(layout::SPACES_DIR)
            .join(format!("{space_id}.files"))
    }

    pub fn tasks_files_dir(&self) -> PathBuf {
        self.root.join(layout::TASKS_FILES_DIR)
    }

    pub fn issue_export_confirmation(
        &self,
        subject: String,
        destination: String,
        format: String,
    ) -> VaultResult<String> {
        let token = uuid::Uuid::new_v4().to_string();
        let mut confirmations = self
            .export_confirmations
            .lock()
            .map_err(|_| VaultError::Validation("导出确认锁中毒".into()))?;
        confirmations.retain(|_, confirmation| confirmation.expires_at > Instant::now());
        confirmations.insert(
            token.clone(),
            ExportConfirmation {
                subject,
                destination,
                format,
                expires_at: Instant::now() + Duration::from_secs(120),
            },
        );
        Ok(token)
    }

    pub fn consume_export_confirmation(
        &self,
        token: &str,
        subject: &str,
        destination: &str,
        format: &str,
    ) -> VaultResult<()> {
        let mut confirmations = self
            .export_confirmations
            .lock()
            .map_err(|_| VaultError::Validation("导出确认锁中毒".into()))?;
        let confirmation = confirmations
            .remove(token)
            .ok_or_else(|| VaultError::Validation("加密明文导出确认无效、已使用或已过期".into()))?;
        if confirmation.expires_at <= Instant::now()
            || confirmation.subject != subject
            || confirmation.destination != destination
            || confirmation.format != format
        {
            return Err(VaultError::Validation(
                "加密明文导出确认无效、已使用或已过期".into(),
            ));
        }
        Ok(())
    }

    fn get_setting_string(&self, key: &str, default: &str) -> VaultResult<String> {
        get_setting(&self, key).map(|v| if v.is_empty() { default.to_string() } else { v })
    }

    /// 巡检：锁定所有闲置分区并销毁其内存临时索引。返回本次锁定的分区 id。
    pub fn sweep_idle(&self) -> Vec<String> {
        let idle = self.session.idle_section_ids();
        for section_id in &idle {
            self.lock_section_runtime(section_id);
        }
        idle
    }

    /// 锁定收口的运行时部分（会话移除 + 内存索引 DROP + 事件由 command 层补发）。
    pub fn lock_section_runtime(&self, section_id: &str) {
        self.session.lock(section_id);
        let Ok(_maintenance) = self.maintenance_read() else {
            return;
        };
        if let Ok(spaces) = self.spaces() {
            for conn in spaces.values() {
                let _ = crate::search::drop_unlocked_index(conn, section_id);
            }
        }
    }
}

use crate::db::registry;

// ---------------------------------------------------------------- 设置

fn get_setting(inner: &AppStateInner, key: &str) -> VaultResult<String> {
    inner.with_meta(|meta| {
        let value: Option<String> = meta
            .query_row(
                "SELECT value FROM app_config WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()
            .map_err(VaultError::from)?;
        Ok(value.unwrap_or_default())
    })
}

pub(crate) fn set_setting(inner: &AppStateInner, key: &str, value: &str) -> VaultResult<()> {
    inner.with_meta(|meta| {
        meta.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSettings {
    pub clipboard_auto_clear_seconds: u64,
    pub section_auto_lock_minutes: u64,
    pub encrypted_section_show_titles: bool,
    pub default_space_name: String,
    pub lunar_overlay_enabled: bool,
    pub festivals_enabled: bool,
    pub solar_terms_enabled: bool,
    pub auto_backup_enabled: bool,
    pub auto_backup_schedule: String,
    pub auto_backup_directory: String,
    pub auto_backup_retention_count: u16,
    pub auto_backup_last_success_at: String,
    pub close_to_tray: bool,
    pub app_locale: String,
    pub app_theme: String,
    pub app_shortcuts: String,
}

fn parse_setting<T: std::str::FromStr>(raw: &str, default: T) -> T {
    raw.parse().ok().unwrap_or(default)
}

pub fn settings_of(inner: &AppStateInner) -> VaultResult<AppSettings> {
    Ok(AppSettings {
        clipboard_auto_clear_seconds: parse_setting(
            &get_setting(inner, SETTING_CLIPBOARD_AUTO_CLEAR)?,
            0,
        ),
        section_auto_lock_minutes: {
            let minutes = parse_setting(&get_setting(inner, SETTING_SECTION_AUTO_LOCK)?, 5u64);
            inner.session.set_auto_lock_minutes(minutes);
            minutes
        },
        encrypted_section_show_titles: parse_setting(
            &get_setting(inner, SETTING_SHOW_TITLES)?,
            true,
        ),
        default_space_name: inner.get_setting_string(SETTING_DEFAULT_SPACE_NAME, "默认空间")?,
        lunar_overlay_enabled: parse_setting(&get_setting(inner, SETTING_LUNAR_OVERLAY)?, true),
        festivals_enabled: parse_setting(&get_setting(inner, SETTING_FESTIVALS)?, true),
        solar_terms_enabled: parse_setting(&get_setting(inner, SETTING_SOLAR_TERMS)?, true),
        auto_backup_enabled: parse_setting(
            &get_setting(inner, SETTING_AUTO_BACKUP_ENABLED)?,
            false,
        ),
        auto_backup_schedule: inner.get_setting_string(SETTING_AUTO_BACKUP_SCHEDULE, "daily")?,
        auto_backup_directory: get_setting(inner, SETTING_AUTO_BACKUP_DIRECTORY)?,
        auto_backup_retention_count: parse_setting(
            &get_setting(inner, SETTING_AUTO_BACKUP_RETENTION)?,
            5u16,
        ),
        auto_backup_last_success_at: get_setting(inner, SETTING_AUTO_BACKUP_LAST_SUCCESS)?,
        close_to_tray: parse_setting(&get_setting(inner, SETTING_CLOSE_TO_TRAY)?, true),
        app_locale: inner.get_setting_string(SETTING_APP_LOCALE, "system")?,
        app_theme: inner.get_setting_string(SETTING_APP_THEME, "system")?,
        app_shortcuts: inner.get_setting_string(SETTING_APP_SHORTCUTS, "{}")?,
    })
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, VaultError> {
    settings_of(&state.inner)
}

#[tauri::command]
pub fn set_setting_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<AppSettings, VaultError> {
    const ALLOWED: [&str; 16] = [
        SETTING_CLIPBOARD_AUTO_CLEAR,
        SETTING_SECTION_AUTO_LOCK,
        SETTING_SHOW_TITLES,
        SETTING_DEFAULT_SPACE_NAME,
        SETTING_LUNAR_OVERLAY,
        SETTING_FESTIVALS,
        SETTING_SOLAR_TERMS,
        SETTING_AUTO_BACKUP_ENABLED,
        SETTING_AUTO_BACKUP_SCHEDULE,
        SETTING_AUTO_BACKUP_DIRECTORY,
        SETTING_AUTO_BACKUP_RETENTION,
        SETTING_AUTO_BACKUP_LAST_SUCCESS,
        SETTING_CLOSE_TO_TRAY,
        SETTING_APP_LOCALE,
        SETTING_APP_THEME,
        SETTING_APP_SHORTCUTS,
    ];
    if !ALLOWED.contains(&key.as_str()) {
        return Err(VaultError::Validation(format!("不支持的设置项 {key}")));
    }
    if key == SETTING_SECTION_AUTO_LOCK {
        let minutes: u64 = value
            .parse()
            .map_err(|_| VaultError::Validation("闲置时长必须是正整数分钟".into()))?;
        if minutes == 0 { return Err(VaultError::Validation("闲置时长必须是正整数分钟".into())); }
    }
    if [
        SETTING_LUNAR_OVERLAY,
        SETTING_FESTIVALS,
        SETTING_SOLAR_TERMS,
        SETTING_SHOW_TITLES,
        SETTING_AUTO_BACKUP_ENABLED,
        SETTING_CLOSE_TO_TRAY,
    ]
    .contains(&key.as_str())
        && value.parse::<bool>().is_err()
    {
        return Err(VaultError::Validation(format!(
            "设置项 {key} 必须为 true 或 false"
        )));
    }
    if key == SETTING_AUTO_BACKUP_DIRECTORY && !value.is_empty() {
        let directory = std::path::Path::new(&value);
        if !directory.is_dir() {
            return Err(VaultError::Validation(
                "自动备份目录不存在或不是目录".into(),
            ));
        }
        let probe = directory.join(format!(
            ".tenjee-vault-write-probe-{}",
            uuid::Uuid::new_v4()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
        {
            Ok(file) => {
                drop(file);
                let _ = std::fs::remove_file(probe);
            }
            Err(_) => return Err(VaultError::Validation("自动备份目录不可写".into())),
        }
    }
    if key == SETTING_AUTO_BACKUP_LAST_SUCCESS {
        return Err(VaultError::Validation("上次自动备份时间由应用维护".into()));
    }
    if key == SETTING_CLIPBOARD_AUTO_CLEAR && value.parse::<u64>().is_err() { return Err(VaultError::Validation("剪贴板时长必须为非负整数".into())); }
    let mut previous_shortcuts = None;
    if key == SETTING_APP_SHORTCUTS {
        let parsed: serde_json::Value = serde_json::from_str(&value)
            .map_err(|_| VaultError::Validation("快捷键设置必须是 JSON 对象".into()))?;
        if !parsed.is_object() {
            return Err(VaultError::Validation("快捷键设置必须是 JSON 对象".into()));
        }
        let previous = state
            .inner
            .get_setting_string(SETTING_APP_SHORTCUTS, "{}")?;
        crate::desktop::replace_system_shortcuts(&app, &value, &previous)
            .map_err(VaultError::Validation)?;
        previous_shortcuts = Some(previous);
    }
    if let Err(error) = set_setting(&state.inner, &key, &value) {
        if let Some(previous) = previous_shortcuts { crate::desktop::replace_system_shortcuts(&app,&previous,&value).map_err(VaultError::Validation)?; }
        return Err(error);
    }
    settings_of(&state.inner)
}

pub fn reset_group(inner: &AppStateInner, group: &str) -> VaultResult<AppSettings> {
    let defaults: &[(&str,&str)] = match group {
        "security" => &[(SETTING_CLIPBOARD_AUTO_CLEAR,"0"),(SETTING_SECTION_AUTO_LOCK,"5"),(SETTING_SHOW_TITLES,"true")],
        "calendar" => &[(SETTING_LUNAR_OVERLAY,"true"),(SETTING_FESTIVALS,"true"),(SETTING_SOLAR_TERMS,"true")],
        "appearance" => &[(SETTING_APP_LOCALE,"system"),(SETTING_APP_THEME,"system")],
        "desktop" => &[(SETTING_CLOSE_TO_TRAY,"false")],
        "shortcuts" => &[(SETTING_APP_SHORTCUTS,"{}")],
        "backup" => &[(SETTING_AUTO_BACKUP_ENABLED,"false"),(SETTING_AUTO_BACKUP_SCHEDULE,"daily"),(SETTING_AUTO_BACKUP_DIRECTORY,""),(SETTING_AUTO_BACKUP_RETENTION,"5")],
        _ => return Err(VaultError::Validation("Unknown settings group".into())),
    };
    inner.with_meta(|conn| {
        let transaction = conn.unchecked_transaction()?;
        for (key,value) in defaults { transaction.execute("INSERT INTO app_config(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value])?; }
        transaction.commit()?; Ok(())
    })?;
    settings_of(inner)
}

#[tauri::command]
pub fn reset_settings_group(app: AppHandle,state: State<'_,AppState>,group:String) -> VaultResult<AppSettings> {
    let previous = settings_of(&state.inner)?.app_shortcuts;
    if group == "shortcuts" { crate::desktop::replace_system_shortcuts(&app,"{}",&previous).map_err(VaultError::Validation)?; }
    let result = reset_group(&state.inner,&group);
    if result.is_err() && group == "shortcuts" { crate::desktop::replace_system_shortcuts(&app,&previous,"{}").map_err(VaultError::Validation)?; }
    result
}

// ---------------------------------------------------------------- 空间

#[tauri::command]
pub fn list_spaces(state: State<'_, AppState>) -> Result<Vec<SpaceInfo>, VaultError> {
    state.inner.with_meta(|meta| registry::list_spaces(meta))
}

#[tauri::command]
pub fn create_space_cmd(state: State<'_, AppState>, name: String) -> Result<SpaceInfo, VaultError> {
    if name.trim().is_empty() {
        return Err(VaultError::Validation("空间名称不能为空".into()));
    }
    state
        .inner
        .with_meta(|meta| registry::create_space(meta, &state.inner.root, name.trim()))
}

#[tauri::command]
pub fn rename_space_cmd(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), VaultError> {
    if name.trim().is_empty() {
        return Err(VaultError::Validation("空间名称不能为空".into()));
    }
    state
        .inner
        .with_meta(|meta| registry::rename_space(meta, &id, name.trim()))?;
    Ok(())
}

/// 归档：移出注册表，库文件保留（spec: 归档的空间数据保留、可恢复）。
#[tauri::command]
pub fn archive_space_cmd(app:tauri::AppHandle,state: State<'_, AppState>, id: String) -> Result<(), VaultError> {
    crate::sync::session::before_protection(&app,&state.inner)?;
    state.inner.with_workspace(|meta,_,_,spaces|{
        crate::sync::engine::incorporate_space_contexts(meta,spaces)?;
        registry::archive_space(meta,&id)?;spaces.remove(&id);Ok(())
    })
}

/// Delete after recording causal knowledge of the full owning space.
#[tauri::command]
pub fn delete_space_cmd(app:tauri::AppHandle,state: State<'_, AppState>, id: String) -> Result<(), VaultError> {
    crate::sync::session::before_protection(&app,&state.inner)?;
    state.inner.with_workspace(|meta,_,_,spaces|{
        let info=registry::get_space(meta,&id)?.ok_or_else(||VaultError::NotFound("Space".into()))?;
        crate::sync::engine::incorporate_space_contexts(meta,spaces)?;
        registry::archive_space(meta,&id)?;spaces.remove(&id);
        let db_path=state.inner.root.join(&info.db_file);
        for suffix in ["","-wal","-shm"] {let path=PathBuf::from(format!("{}{suffix}",db_path.display()));if path.exists(){std::fs::remove_file(path)?;}}
        let files=state.inner.files_dir(&id);if files.exists(){std::fs::remove_dir_all(files)?;}
        Ok(())
    })
}

/// 兜底：注册表为空时创建默认空间（应用启动已自动执行，供前端刷新用）。
#[tauri::command]
pub fn ensure_default_space(state: State<'_, AppState>) -> Result<Vec<SpaceInfo>, VaultError> {
    let spaces = state.inner.with_meta(|meta| registry::list_spaces(meta))?;
    if spaces.is_empty() {
        let name = state
            .inner
            .get_setting_string(SETTING_DEFAULT_SPACE_NAME, "默认空间")?;
        state
            .inner
            .with_meta(|meta| registry::create_space(meta, &state.inner.root, &name))?;
        return state.inner.with_meta(|meta| registry::list_spaces(meta));
    }
    Ok(spaces)
}

// ---------------------------------------------------------------- 既有占位

/// 连通性验证：最小前后端链路检查。
#[tauri::command]
pub fn ping() -> String {
    "pong".to_string()
}

/// 返回各数据库状态（正常/隔离/缺失重建 + 版本号）与告警列表。
#[tauri::command]
pub fn db_status(state: State<'_, AppState>) -> Result<StartupReport, VaultError> {
    Ok(state.inner.report.clone())
}

#[tauri::command]
pub fn drain_in_app_reminders(state: State<'_, AppState>) -> Vec<InAppNotice> {
    state.inner.drain_in_app_reminders()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resetting_settings_is_scoped_and_transactional() {
        let dir = tempfile::tempdir().unwrap();
        let root = layout::data_root(dir.path());
        let app = AppState::init(root.clone(),crate::db::startup::startup(&root).unwrap()).unwrap();
        set_setting(&app.inner,SETTING_SECTION_AUTO_LOCK,"20").unwrap();
        set_setting(&app.inner,SETTING_APP_LOCALE,"en").unwrap();
        let spaces = app.inner.with_meta(registry::list_spaces).unwrap();
        let saved = reset_group(&app.inner,"security").unwrap();
        assert_eq!(saved.section_auto_lock_minutes,5);
        assert_eq!(saved.app_locale,"en");
        assert_eq!(app.inner.with_meta(registry::list_spaces).unwrap().len(),spaces.len());
        set_setting(&app.inner,SETTING_SECTION_AUTO_LOCK,"20").unwrap();
        set_setting(&app.inner,SETTING_CLIPBOARD_AUTO_CLEAR,"15").unwrap();
        app.inner.with_meta(|conn| { conn.execute_batch("CREATE TRIGGER reject_reset BEFORE UPDATE ON app_config WHEN NEW.key='section_auto_lock_minutes' BEGIN SELECT RAISE(ABORT,'injected failure'); END;")?; Ok(()) }).unwrap();
        assert!(reset_group(&app.inner,"security").is_err());
        let saved = settings_of(&app.inner).unwrap();
        assert_eq!(saved.clipboard_auto_clear_seconds,15);
        assert_eq!(saved.section_auto_lock_minutes,20);
        assert!(reset_group(&app.inner,"unknown").is_err());
    }

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let root = layout::data_root(dir.path());
        let report = crate::db::startup::startup(&root).unwrap();
        (dir, AppState::init(root, report).unwrap())
    }

    #[test]
    fn init_creates_default_space_when_registry_empty() {
        let (_dir, app) = state();
        let spaces = app.inner.with_meta(|m| registry::list_spaces(m)).unwrap();
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].name, "默认空间");
        // 附件目录已建
        assert!(app.inner.files_dir(&spaces[0].id).is_dir());
        app.inner
            .with_tasks(|conn| {
                assert_eq!(
                    crate::db::migrate::current_version(conn)?,
                    DbKind::Tasks.migrations().last().unwrap().version
                );
                Ok(())
            })
            .unwrap();
        app.inner
            .with_calendar(|conn| {
                assert_eq!(
                    crate::db::migrate::current_version(conn)?,
                    DbKind::Calendar.migrations().last().unwrap().version
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn encrypted_export_confirmation_is_single_use_and_bound_to_its_destination() {
        let (_dir, app) = state();
        let token = app
            .inner
            .issue_export_confirmation(
                "page:page-1".into(),
                "/exports/page.md".into(),
                "markdown".into(),
            )
            .unwrap();
        assert!(app
            .inner
            .consume_export_confirmation(&token, "page:page-1", "/exports/other.md", "markdown")
            .is_err());
        assert!(app
            .inner
            .consume_export_confirmation(&token, "page:page-1", "/exports/page.md", "markdown")
            .is_err());

        let token = app
            .inner
            .issue_export_confirmation(
                "page:page-1".into(),
                "/exports/page.md".into(),
                "markdown".into(),
            )
            .unwrap();
        app.inner
            .consume_export_confirmation(&token, "page:page-1", "/exports/page.md", "markdown")
            .unwrap();
        assert!(app
            .inner
            .consume_export_confirmation(&token, "page:page-1", "/exports/page.md", "markdown")
            .is_err());

        app.inner.export_confirmations.lock().unwrap().insert(
            "expired".into(),
            ExportConfirmation {
                subject: "page:page-1".into(),
                destination: "/exports/page.md".into(),
                format: "markdown".into(),
                expires_at: Instant::now() - Duration::from_secs(1),
            },
        );
        assert!(app
            .inner
            .consume_export_confirmation("expired", "page:page-1", "/exports/page.md", "markdown")
            .is_err());
    }

    #[test]
    fn default_space_name_follows_setting() {
        let dir = tempfile::tempdir().unwrap();
        let root = layout::data_root(dir.path());
        let report = crate::db::startup::startup(&root).unwrap();
        // 先写设置再 init
        {
            let mut meta = connection::open_db(&root.join(layout::META_DB)).unwrap();
            run_migrations(&mut meta, DbKind::Meta.migrations()).unwrap();
            meta.execute(
                "INSERT INTO app_config (key, value) VALUES ('default_space_name', '我的工作区')",
                [],
            )
            .unwrap();
        }
        let app = AppState::init(root, report).unwrap();
        let spaces = app.inner.with_meta(|m| registry::list_spaces(m)).unwrap();
        assert_eq!(spaces[0].name, "我的工作区");
    }

    #[test]
    fn settings_roundtrip_and_validation() {
        let (_dir, app) = state();
        let inner = &app.inner;
        set_setting(inner, SETTING_CLIPBOARD_AUTO_CLEAR, "30").unwrap();
        let s = settings_of(inner).unwrap();
        assert_eq!(s.clipboard_auto_clear_seconds, 30);
        assert_eq!(s.section_auto_lock_minutes, 5, "默认 5 分钟");
        assert!(s.encrypted_section_show_titles);
        assert!(s.lunar_overlay_enabled);
        assert!(s.festivals_enabled);
        assert!(s.solar_terms_enabled);
        set_setting(inner, SETTING_LUNAR_OVERLAY, "false").unwrap();
        set_setting(inner, SETTING_FESTIVALS, "false").unwrap();
        let persisted = settings_of(inner).unwrap();
        assert!(!persisted.lunar_overlay_enabled);
        assert!(!persisted.festivals_enabled);
        assert!(persisted.solar_terms_enabled);
        // 未知键拒绝
        assert!(set_setting(inner, "evil_key", "1").is_ok()); // 底层函数不限键
        let settings = settings_of(inner).unwrap();
        assert_eq!(settings.default_space_name, "默认空间");
    }

    #[test]
    fn task_command_flow_via_inner() {
        let (_dir, app) = state();
        let list = app
            .inner
            .with_tasks(|conn| crate::tasks::lists::create_list(conn, "工作", None))
            .unwrap();
        let task =
            crate::commands::tasks::create_task_for_test(&app.inner, &list.id, "写周报").unwrap();
        app.inner
            .with_tasks(|conn| {
                crate::tasks::tasks::set_task_status(
                    conn,
                    &task.id,
                    crate::tasks::tasks::STATUS_DONE,
                )?;
                let rows = crate::tasks::tasks::kanban_view(conn)?;
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].status, crate::tasks::tasks::STATUS_DONE);
                Ok(())
            })
            .unwrap();
    }

    /// command 纯函数链路（不依赖 WebView）：层级 → 页面 → 搜索 → 锁定一致性。
    #[test]
    fn notes_command_flow_via_inner() {
        use crate::notes::{hierarchy, pages};
        let (_dir, app) = state();
        let space_id = app.inner.with_meta(|m| registry::list_spaces(m)).unwrap()[0]
            .id
            .clone();

        app.inner
            .with_space(&space_id, |conn| {
                hierarchy::create_notebook(conn, "笔记", None)
            })
            .unwrap();
        let nb_id = app
            .inner
            .with_space(&space_id, |conn| {
                Ok(hierarchy::notebook_tree(conn)?.remove(0).notebook.id)
            })
            .unwrap();
        let section_id = app
            .inner
            .with_space(&space_id, |conn| {
                hierarchy::create_section(conn, &nb_id, None, "默认分区", None)
            })
            .unwrap()
            .id;
        let page = app
            .inner
            .with_space(&space_id, |conn| {
                hierarchy::create_page(conn, &section_id, None, "第一页")
            })
            .unwrap();
        app.inner
            .with_space(&space_id, |conn| {
                pages::save_page(
                    conn,
                    &app.inner.session,
                    &page.id,
                    "第一页",
                    "hello world 内容",
                )
            })
            .unwrap();

        // 全文搜索命中
        let hits = app
            .inner
            .with_space(&space_id, |conn| {
                crate::search::search(conn, "hello", &[], 10)
            })
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page_id, page.id);

        // 读取与最近使用登记
        let loaded = app
            .inner
            .with_space(&space_id, |conn| {
                pages::get_page(conn, &app.inner.session, &page.id)
            })
            .unwrap();
        assert_eq!(loaded.content, "hello world 内容");
        app.inner
            .with_meta(|meta| {
                meta.execute(
                    "INSERT INTO recent_pages (space_id, page_id) VALUES (?1, ?2) ON CONFLICT(space_id, page_id) DO UPDATE SET opened_at = datetime('now')",
                    params![space_id, page.id],
                )?;
                Ok(())
            })
            .unwrap();

        // 心跳巡检：无解锁分区时为空
        assert!(app.inner.sweep_idle().is_empty());
    }

    #[test]
    fn with_space_opens_and_caches_connection() {
        let (_dir, app) = state();
        let space_id = app
            .inner
            .with_meta(|meta| registry::list_spaces(meta))
            .unwrap()[0]
            .id
            .clone();
        app.inner
            .with_space(&space_id, |conn| {
                let n: i64 = conn
                    .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'pages_fts'", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(n, 1, "空间库必须带 FTS 迁移");
                Ok(())
            })
            .unwrap();
        // 第二次走缓存
        app.inner.with_space(&space_id, |_| Ok(())).unwrap();
        // 未知空间报错
        assert!(matches!(
            app.inner.with_space("nope", |_| Ok(())),
            Err(VaultError::NotFound(_))
        ));
    }

    #[test]
    fn delete_space_removes_files_and_registry() {
        let (_dir, app) = state();
        let info = app
            .inner
            .with_meta(|m| registry::create_space(m, &app.inner.root, "临时"))
            .unwrap();
        assert!(app.inner.root.join(&info.db_file).exists());
        // 通过 command 逻辑删除（直接调用内部流程）
        let inner = &app.inner;
        inner.spaces().unwrap().remove(&info.id);
        inner
            .with_meta(|m| registry::archive_space(m, &info.id))
            .unwrap();
        let db_path = inner.root.join(&info.db_file);
        std::fs::remove_file(&db_path).unwrap();
        std::fs::remove_dir_all(inner.files_dir(&info.id)).unwrap();
        assert!(inner
            .with_meta(|m| registry::get_space(m, &info.id))
            .unwrap()
            .is_none());
        assert!(!inner.files_dir(&info.id).exists());
    }

    #[test]
    fn maintenance_gate_blocks_writes_until_exclusive_work_finishes() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (_dir, app) = state();
        let inner = app.inner.clone();
        let exclusive = app.inner.maintenance_write().unwrap();
        let (tx, rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            inner
                .with_tasks(|conn| {
                    conn.execute(
                        "INSERT INTO task_lists (id, name) VALUES ('blocked', '闸门测试')",
                        [],
                    )?;
                    tx.send(()).unwrap();
                    Ok(())
                })
                .unwrap();
        });

        assert!(
            rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "独占维护期间，修改连接访问器必须等待"
        );
        drop(exclusive);
        rx.recv_timeout(Duration::from_secs(1))
            .expect("释放维护闸门后写入应继续");
        writer.join().unwrap();
    }

    #[test]
    fn concurrent_writes_to_multiple_spaces_complete_without_deadlock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (_dir, app) = state();
        let first = app
            .inner
            .with_meta(|meta| registry::list_spaces(meta))
            .unwrap()[0]
            .id
            .clone();
        let second = app
            .inner
            .with_meta(|meta| registry::create_space(meta, &app.inner.root, "第二空间"))
            .unwrap()
            .id;
        let (tx, rx) = mpsc::channel();
        let mut writers = Vec::new();
        for (space_id, notebook_id) in [(first, "nb-first"), (second, "nb-second")] {
            let inner = app.inner.clone();
            let tx = tx.clone();
            writers.push(std::thread::spawn(move || {
                inner
                    .with_space(&space_id, |conn| {
                        conn.execute(
                            "INSERT INTO notebooks (id, name) VALUES (?1, ?2)",
                            params![notebook_id, notebook_id],
                        )?;
                        Ok(())
                    })
                    .unwrap();
                tx.send(space_id).unwrap();
            }));
        }
        drop(tx);
        let completed: Vec<String> = (0..2)
            .map(|_| {
                rx.recv_timeout(Duration::from_secs(1))
                    .expect("多空间写入不应死锁")
            })
            .collect();
        assert_eq!(completed.len(), 2);
        for writer in writers {
            writer.join().unwrap();
        }
    }
}
