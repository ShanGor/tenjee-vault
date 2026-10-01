//! 启动流程：布局初始化 → 打开主库并迁移 → 完整性检查 → 加载空间注册表 → 逐个打开空间库。
//! 任一非主库失败仅隔离该库并产生告警，绝不阻断其余库或使进程退出。

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::connection;
use super::layout;
use super::migrate::{current_version, run_migrations, DbKind};
use super::registry;
use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbHealth {
    /// 正常，附已应用最新迁移版本
    Ok { version: u64 },
    /// 文件缺失，已自动重建（含 schema）
    Rebuilt { version: u64 },
    /// 完整性检查失败或无法打开，已隔离
    Isolated { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbStatusEntry {
    pub name: String,
    pub health: DbHealth,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StartupReport {
    pub dbs: Vec<DbStatusEntry>,
    pub alerts: Vec<String>,
}

impl StartupReport {
    fn push(&mut self, name: &str, health: DbHealth) {
        match &health {
            DbHealth::Isolated { reason } => {
                self.alerts.push(format!("数据库 {name} 已隔离: {reason}"))
            }
            DbHealth::Rebuilt { .. } => self.alerts.push(format!("数据库 {name} 缺失，已自动重建")),
            DbHealth::Ok { .. } => {}
        }
        self.dbs.push(DbStatusEntry {
            name: name.to_string(),
            health,
        });
    }
}

/// 打开一个领域库并迁移 + 完整性检查。返回 (连接, 是否新建, 版本)。
fn open_domain_db(path: &Path, kind: DbKind) -> VaultResult<(Connection, bool, u64)> {
    let existed = path.exists();
    let mut conn = connection::open_db(path)?;
    run_migrations(&mut conn, kind.migrations())?;
    connection::integrity_check(&conn)?;
    let version = current_version(&conn)?;
    Ok((conn, existed, version))
}

/// 主库打开失败时执行自愈：损坏文件改名后重建（配置重置，领域数据不丢）。
fn recover_meta(root: &Path) -> VaultResult<(Connection, u64)> {
    let path = root.join(layout::META_DB);
    let corrupt = root.join(format!("{}.corrupt", layout::META_DB));
    std::fs::rename(&path, &corrupt)?;
    let mut conn = connection::open_db(&path)?;
    run_migrations(&mut conn, DbKind::Meta.migrations())?;
    let version = current_version(&conn)?;
    Ok((conn, version))
}

/// 执行启动流程，生成结构化报告（供前端展示与告警）。
pub fn startup(root: &Path) -> VaultResult<StartupReport> {
    // 在补齐布局之前记录缺失情况：既非首次运行（meta.db 已存在）而领域库缺失 = 重建
    let first_run = !root.join(layout::META_DB).exists();
    let tasks_missing = !root.join(layout::TASKS_DB).exists();
    let calendar_missing = !root.join(layout::CALENDAR_DB).exists();
    layout::ensure_layout(root)?;
    let mut report = StartupReport::default();

    // 1. 主库：损坏则自愈重建；无法自愈才是致命错误
    let (meta, meta_version, meta_recovered) =
        match open_domain_db(&root.join(layout::META_DB), DbKind::Meta) {
            Ok((conn, existed, version)) => (conn, version, !existed),
            Err(e) => {
                let (conn, version) = recover_meta(root).map_err(|re| {
                    VaultError::DbIntegrity(format!("主库无法恢复: {e}; 重建也失败: {re}"))
                })?;
                (conn, version, true)
            }
        };
    if meta_recovered {
        report.push(
            layout::META_DB,
            DbHealth::Rebuilt {
                version: meta_version,
            },
        );
    } else {
        report.push(
            layout::META_DB,
            DbHealth::Ok {
                version: meta_version,
            },
        );
    }

    // 2. 领域库：单库失败仅隔离；非首次运行的缺失视为「重建」并提示
    for (file, kind, was_missing) in [
        (layout::TASKS_DB, DbKind::Tasks, tasks_missing),
        (layout::CALENDAR_DB, DbKind::Calendar, calendar_missing),
    ] {
        match open_domain_db(&root.join(file), kind) {
            Ok((_, _, version)) => {
                let rebuilt = was_missing && !first_run;
                report.push(
                    file,
                    if rebuilt {
                        DbHealth::Rebuilt { version }
                    } else {
                        DbHealth::Ok { version }
                    },
                );
            }
            Err(e) => report.push(
                file,
                DbHealth::Isolated {
                    reason: e.to_string(),
                },
            ),
        }
    }

    // 3. 空间库：注册表驱动，逐个打开
    for space in registry::list_spaces(&meta)? {
        let path = root.join(&space.db_file);
        if !path.exists() {
            report.push(
                &space.id,
                DbHealth::Isolated {
                    reason: format!("注册表指向的空间库文件缺失 ({})", space.db_file),
                },
            );
            continue;
        }
        match open_domain_db(&path, DbKind::Space) {
            Ok((_, _, version)) => report.push(&space.id, DbHealth::Ok { version }),
            Err(e) => report.push(
                &space.id,
                DbHealth::Isolated {
                    reason: e.to_string(),
                },
            ),
        }
    }
    drop(meta);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root_with_layout() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = layout::data_root(dir.path());
        layout::ensure_layout(&root).unwrap();
        (dir, root)
    }

    #[test]
    fn fresh_startup_all_healthy() {
        let (_dir, root) = root_with_layout();
        let report = startup(&root).unwrap();
        assert_eq!(report.dbs.len(), 3);
        let meta_latest = DbKind::Meta.migrations().last().unwrap().version;
        assert!(
            matches!(report.dbs[0].health, DbHealth::Ok { version } if version == meta_latest),
            "{report:?}"
        );
        assert!(
            report.dbs[1..]
                .iter()
                .zip([DbKind::Tasks, DbKind::Calendar])
                .all(|(d,kind)| matches!(d.health, DbHealth::Ok { version } if version == kind.migrations().last().unwrap().version)),
            "{report:?}"
        );
        assert!(report.alerts.is_empty());
    }

    #[test]
    fn missing_domain_db_rebuilds_with_schema() {
        let (_dir, root) = root_with_layout();
        let mut meta = connection::open_db(&root.join(layout::META_DB)).unwrap();
        run_migrations(&mut meta, DbKind::Meta.migrations()).unwrap();
        drop(meta);
        std::fs::remove_file(root.join(layout::TASKS_DB)).unwrap();
        let report = startup(&root).unwrap();
        let tasks = report
            .dbs
            .iter()
            .find(|d| d.name == layout::TASKS_DB)
            .unwrap();
        let tasks_latest = DbKind::Tasks.migrations().last().unwrap().version;
        assert!(
            matches!(tasks.health, DbHealth::Rebuilt { version } if version == tasks_latest),
            "{report:?}"
        );
        // 重建的库带 schema
        let conn = connection::open_db(&root.join(layout::TASKS_DB)).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'tasks'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        // 其余库不受影响
        let calendar = report
            .dbs
            .iter()
            .find(|d| d.name == layout::CALENDAR_DB)
            .unwrap();
        assert!(matches!(calendar.health, DbHealth::Ok { .. }));
    }

    #[test]
    fn corrupt_domain_db_isolated_others_unaffected() {
        let (_dir, root) = root_with_layout();
        // 先正常启动一次建好 schema
        startup(&root).unwrap();
        // 损坏 calendar.db
        std::fs::write(root.join(layout::CALENDAR_DB), b"garbage bytes, not sqlite").unwrap();
        let report = startup(&root).unwrap();
        let calendar = report
            .dbs
            .iter()
            .find(|d| d.name == layout::CALENDAR_DB)
            .unwrap();
        assert!(
            matches!(calendar.health, DbHealth::Isolated { .. }),
            "{report:?}"
        );
        assert!(report.alerts.iter().any(|a| a.contains("calendar.db")));
        let tasks = report
            .dbs
            .iter()
            .find(|d| d.name == layout::TASKS_DB)
            .unwrap();
        assert!(matches!(tasks.health, DbHealth::Ok { .. }));
        let meta = report
            .dbs
            .iter()
            .find(|d| d.name == layout::META_DB)
            .unwrap();
        assert!(matches!(meta.health, DbHealth::Ok { .. }));
    }

    #[test]
    fn corrupt_meta_recovers_and_alerts() {
        let (_dir, root) = root_with_layout();
        std::fs::write(root.join(layout::META_DB), b"totally broken meta").unwrap();
        let report = startup(&root).unwrap();
        let meta = report
            .dbs
            .iter()
            .find(|d| d.name == layout::META_DB)
            .unwrap();
        assert!(
            matches!(meta.health, DbHealth::Rebuilt { .. }),
            "{report:?}"
        );
        assert!(root.join(format!("{}.corrupt", layout::META_DB)).exists());
        // 领域库仍正常
        assert!(report.dbs.iter().any(|d| d.name == layout::TASKS_DB));
    }

    #[test]
    fn registered_space_missing_file_isolated() {
        let (dir, root) = root_with_layout();
        let mut meta = connection::open_db(&root.join(layout::META_DB)).unwrap();
        run_migrations(&mut meta, DbKind::Meta.migrations()).unwrap();
        let space = registry::create_space(&meta, &root, "s").unwrap();
        drop(meta);
        std::fs::remove_file(root.join(&space.db_file)).unwrap();
        let report = startup(&root).unwrap();
        let entry = report.dbs.iter().find(|d| d.name == space.id).unwrap();
        assert!(
            matches!(entry.health, DbHealth::Isolated { .. }),
            "{report:?}"
        );
        // 其余部分照常
        assert!(report.dbs.iter().any(|d| d.name == layout::TASKS_DB));
        let _ = dir;
    }

    #[test]
    fn transactional_write_rolls_back_on_failure() {
        // 多记录写入必须原子：中途失败不得产生部分写入
        let (_dir, root) = root_with_layout();
        let mut conn = connection::open_db(&root.join(layout::TASKS_DB)).unwrap();
        run_migrations(&mut conn, DbKind::Tasks.migrations()).unwrap();
        let result: VaultResult<()> = conn.transaction().map_err(VaultError::from).and_then(|tx| {
            tx.execute(
                "INSERT INTO task_lists (id, name) VALUES ('l1', 'list')",
                [],
            )?;
            tx.execute(
                "INSERT INTO tasks (id, list_id, title) VALUES ('t1', 'l1', 'task')",
                [],
            )?;
            // 注入失败：违反 CHECK 约束
            tx.execute(
                "INSERT INTO tasks (id, list_id, title, status) VALUES ('t2', 'l1', 'bad', 'nope')",
                [],
            )?;
            tx.commit()?;
            Ok(())
        });
        assert!(matches!(result, Err(VaultError::Sqlite(_))));
        let lists: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_lists", [], |r| r.get(0))
            .unwrap();
        let tasks: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!((lists, tasks), (0, 0), "失败事务不得残留部分写入");
    }
}
