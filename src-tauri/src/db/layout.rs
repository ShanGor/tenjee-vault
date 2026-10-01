//! 数据目录布局：`<base>/tenjee-vault/` 下的主库、领域库、空间库目录与备份目录。
//! 目录或文件缺失时在启动时自动补齐。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::VaultResult;

pub const APP_DIR_NAME: &str = "tenjee-vault";
pub const META_DB: &str = "meta.db";
pub const TASKS_DB: &str = "tasks.db";
pub const CALENDAR_DB: &str = "calendar.db";
pub const SPACES_DIR: &str = "spaces";
pub const BACKUPS_DIR: &str = "backups";
/// 任务附件目录（design D8；spec §3.1 未定义，此为 M3 新增布局约定）
pub const TASKS_FILES_DIR: &str = "tasks.files";

/// 解析应用数据根目录 `<app_data_dir>/tenjee-vault`。
pub fn data_root(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(APP_DIR_NAME)
}

/// 创建（或补齐）数据布局。已存在的文件不改动。
pub fn ensure_layout(root: &Path) -> VaultResult<()> {
    fs::create_dir_all(root)?;
    fs::create_dir_all(root.join(SPACES_DIR))?;
    fs::create_dir_all(root.join(BACKUPS_DIR))?;
    fs::create_dir_all(root.join(TASKS_FILES_DIR))?;
    for db in [META_DB, TASKS_DB, CALENDAR_DB] {
        let path = root.join(db);
        if !path.exists() {
            // 仅创建文件；schema 由启动迁移流程建立
            fs::File::create(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_dir_gets_full_layout() {
        let dir = tempfile::tempdir().unwrap();
        let root = data_root(dir.path());
        ensure_layout(&root).unwrap();
        assert!(root.join(META_DB).exists());
        assert!(root.join(TASKS_DB).exists());
        assert!(root.join(CALENDAR_DB).exists());
        assert!(root.join(SPACES_DIR).is_dir());
        assert!(root.join(BACKUPS_DIR).is_dir());
        assert!(root.join(TASKS_FILES_DIR).is_dir());
    }

    #[test]
    fn rerun_preserves_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = data_root(dir.path());
        ensure_layout(&root).unwrap();
        std::fs::write(root.join(TASKS_DB), b"sentinel").unwrap();
        ensure_layout(&root).unwrap();
        assert_eq!(std::fs::read(root.join(TASKS_DB)).unwrap(), b"sentinel");
    }

    #[test]
    fn missing_domain_db_is_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let root = data_root(dir.path());
        ensure_layout(&root).unwrap();
        std::fs::remove_file(root.join(CALENDAR_DB)).unwrap();
        ensure_layout(&root).unwrap();
        assert!(root.join(CALENDAR_DB).exists());
    }
}
