//! 空间注册表：meta.db 中的 spaces 表 + `spaces/<id>.db` 文件的一对一管理。
//! 跨库一致性采用「主操作 + 补偿」：注册项与库文件不一致时检测并提示。

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::connection;
use super::migrate::{run_migrations, DbKind};
use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceInfo {
    pub id: String,
    pub name: String,
    pub db_file: String,
}

fn spaces_dir(root: &Path) -> PathBuf {
    root.join(super::layout::SPACES_DIR)
}

/// 创建空间：生成 id、创建库文件（含迁移）与附件目录，登记注册表（单事务）。
pub fn create_space(meta: &Connection, root: &Path, name: &str) -> VaultResult<SpaceInfo> {
    let id = uuid::Uuid::new_v4().to_string();
    let rel = format!("{}/{}.db", super::layout::SPACES_DIR, id);
    let db_path = root.join(&rel);
    let files_dir = spaces_dir(root).join(format!("{}.files", id));
    std::fs::create_dir_all(&files_dir)?;
    let mut conn = connection::open_db(&db_path)?;
    run_migrations(&mut conn, DbKind::Space.migrations())?;
    drop(conn);
    meta.execute(
        "INSERT INTO spaces (id, name, db_file) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, name, rel],
    )?;
    Ok(SpaceInfo {
        id,
        name: name.to_string(),
        db_file: rel,
    })
}

/// 重命名空间。
pub fn rename_space(meta: &Connection, id: &str, new_name: &str) -> VaultResult<()> {
    let n = meta.execute(
        "UPDATE spaces SET name = ?1 WHERE id = ?2",
        rusqlite::params![new_name, id],
    )?;
    if n == 0 {
        return Err(VaultError::NotFound(format!("空间 {id}")));
    }
    Ok(())
}

/// 归档空间：移除注册项；库文件保留在磁盘上（由用户决定删除或恢复）。
pub fn archive_space(meta: &Connection, id: &str) -> VaultResult<()> {
    let n = meta.execute("DELETE FROM spaces WHERE id = ?1", rusqlite::params![id])?;
    if n == 0 {
        return Err(VaultError::NotFound(format!("空间 {id}")));
    }
    Ok(())
}

/// 全部已登记空间。
pub fn list_spaces(meta: &Connection) -> VaultResult<Vec<SpaceInfo>> {
    let mut stmt =
        meta.prepare("SELECT id, name, db_file FROM spaces ORDER BY sort_order, created_at")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SpaceInfo {
                id: r.get(0)?,
                name: r.get(1)?,
                db_file: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 注册表指向但文件缺失的空间。
pub fn missing_spaces(meta: &Connection, root: &Path) -> VaultResult<Vec<SpaceInfo>> {
    Ok(list_spaces(meta)?
        .into_iter()
        .filter(|s| !root.join(&s.db_file).exists())
        .collect())
}

/// 按 id 查空间（不存在返回 None）。
pub fn get_space(meta: &Connection, id: &str) -> VaultResult<Option<SpaceInfo>> {
    let row = meta
        .query_row(
            "SELECT id, name, db_file FROM spaces WHERE id = ?1",
            rusqlite::params![id],
            |r| {
                Ok(SpaceInfo {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    db_file: r.get(2)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::super::migrate::DbKind;
    use super::*;

    /// 建 meta 库 + v1 迁移的临时环境
    fn setup() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let mut meta =
            connection::open_db(&dir.path().join(super::super::layout::META_DB)).unwrap();
        run_migrations(&mut meta, DbKind::Meta.migrations()).unwrap();
        (dir, meta)
    }

    #[test]
    fn create_registers_and_creates_files() {
        let (dir, meta) = setup();
        let root = dir.path();
        let info = create_space(&meta, root, "个人").unwrap();
        assert!(root.join(&info.db_file).exists());
        assert!(super::spaces_dir(root)
            .join(format!("{}.files", info.id))
            .is_dir());
        assert_eq!(list_spaces(&meta).unwrap().len(), 1);
    }

    #[test]
    fn create_space_db_has_schema() {
        let (dir, meta) = setup();
        let info = create_space(&meta, dir.path(), "s").unwrap();
        let conn = connection::open_db(&dir.path().join(&info.db_file)).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sections'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn rename_space_updates_registry() {
        let (dir, meta) = setup();
        let info = create_space(&meta, dir.path(), "旧名").unwrap();
        rename_space(&meta, &info.id, "新名").unwrap();
        assert_eq!(get_space(&meta, &info.id).unwrap().unwrap().name, "新名");
    }

    #[test]
    fn rename_missing_space_errors() {
        let (_dir, meta) = setup();
        assert!(matches!(
            rename_space(&meta, "nope", "x"),
            Err(VaultError::NotFound(_))
        ));
    }

    #[test]
    fn archive_removes_registration_keeps_file() {
        let (dir, meta) = setup();
        let info = create_space(&meta, dir.path(), "s").unwrap();
        archive_space(&meta, &info.id).unwrap();
        assert!(get_space(&meta, &info.id).unwrap().is_none());
        assert!(dir.path().join(&info.db_file).exists(), "归档应保留库文件");
    }

    #[test]
    fn missing_file_detected() {
        let (dir, meta) = setup();
        let info = create_space(&meta, dir.path(), "s").unwrap();
        std::fs::remove_file(dir.path().join(&info.db_file)).unwrap();
        let missing = missing_spaces(&meta, dir.path()).unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].id, info.id);
    }
}
