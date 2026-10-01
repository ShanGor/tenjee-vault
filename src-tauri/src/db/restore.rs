//! Two-phase restore orchestration.  Runtime preparation never mutates the live data root;
//! startup performs the only directory exchange before opening application connections.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backup::reader;
use crate::db::{
    connection, layout,
    migrate::{run_migrations, DbKind},
    registry,
};
use crate::error::{VaultError, VaultResult};

const MARKER: &str = "pending-restore.json";
const DIAGNOSTIC: &str = "restore-diagnostic.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRestore {
    pub nonce: String,
    /// A basename only, relative to the data-root parent.  No archive path, key or user data is
    /// persisted in the marker.
    pub staging_dir: String,
}

/// A deliberately content-free record of the last failed restore.  It survives restart so the
/// settings UI can explain why the original data was kept without ever storing backup content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreDiagnostic {
    pub nonce: String,
    pub failed_candidate: String,
    pub message: String,
    pub occurred_at: String,
}

fn parent_of(root: &Path) -> VaultResult<&Path> {
    root.parent()
        .ok_or_else(|| VaultError::Validation("数据根目录缺少父目录".into()))
}

fn marker_path(root: &Path) -> VaultResult<PathBuf> {
    Ok(parent_of(root)?.join(MARKER))
}
fn diagnostic_path(root: &Path) -> VaultResult<PathBuf> {
    Ok(parent_of(root)?.join(DIAGNOSTIC))
}

fn write_diagnostic(root: &Path, diagnostic: &RestoreDiagnostic) -> VaultResult<()> {
    let parent = parent_of(root)?;
    let temporary = parent.join(format!(
        ".restore-diagnostic-{}.partial",
        uuid::Uuid::new_v4()
    ));
    fs::write(
        &temporary,
        serde_json::to_vec(diagnostic)
            .map_err(|error| VaultError::Validation(format!("无法编码恢复诊断: {error}")))?,
    )?;
    fs::rename(temporary, diagnostic_path(root)?)?;
    Ok(())
}

pub fn read_diagnostic(root: &Path) -> VaultResult<Option<RestoreDiagnostic>> {
    let path = diagnostic_path(root)?;
    if !path.exists() {
        return Ok(None);
    }
    serde_json::from_slice(&fs::read(path)?)
        .map(Some)
        .map_err(|error| VaultError::Validation(format!("恢复诊断无法解析: {error}")))
}

/// Delete only managed recovery copies after the user explicitly asks.  The generated nonce
/// namespace prevents this from touching arbitrary sibling folders.
pub fn clear_pre_restore_copies(root: &Path) -> VaultResult<usize> {
    let parent = parent_of(root)?;
    let prefix = format!("{}-pre-restore-", layout::APP_DIR_NAME);
    let mut removed = 0;
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(&prefix) && entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn safe_staging(parent: &Path, name: &str) -> VaultResult<PathBuf> {
    let path = Path::new(name);
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
        || !name.starts_with(".restore-staging-")
    {
        return Err(VaultError::Validation("恢复暂存目录标记非法".into()));
    }
    Ok(parent.join(path))
}

pub fn prepare_restore(root: &Path, archive: &Path) -> VaultResult<PendingRestore> {
    let parent = parent_of(root)?;
    if marker_path(root)?.exists() {
        return Err(VaultError::Validation(
            "已有待完成恢复；请先重启应用处理".into(),
        ));
    }
    let nonce = uuid::Uuid::new_v4().to_string();
    let staging_name = format!(".restore-staging-{nonce}");
    let staging = parent.join(&staging_name);
    let result = (|| -> VaultResult<PendingRestore> {
        let manifest = reader::extract_verified(archive, &staging)?;
        reader::validate_staging(&staging, &manifest)?;
        let marker = PendingRestore {
            nonce,
            staging_dir: staging_name,
        };
        let bytes = serde_json::to_vec(&marker)
            .map_err(|error| VaultError::Validation(format!("无法编码恢复标记: {error}")))?;
        let temp = parent.join(format!(".pending-restore-{}.partial", uuid::Uuid::new_v4()));
        fs::write(&temp, bytes)?;
        fs::rename(temp, marker_path(root)?)?;
        Ok(marker)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

fn validate_after_install(root: &Path) -> VaultResult<()> {
    let mut meta = connection::open_db(&root.join(layout::META_DB))?;
    run_migrations(&mut meta, DbKind::Meta.migrations())?;
    injected_failure("during_migration")?;
    connection::integrity_check(&meta)?;
    injected_failure("during_integrity_check")?;
    let mut tasks = connection::open_db(&root.join(layout::TASKS_DB))?;
    run_migrations(&mut tasks, DbKind::Tasks.migrations())?;
    connection::integrity_check(&tasks)?;
    let mut calendar = connection::open_db(&root.join(layout::CALENDAR_DB))?;
    run_migrations(&mut calendar, DbKind::Calendar.migrations())?;
    connection::integrity_check(&calendar)?;
    for space in registry::list_spaces(&meta)? {
        let mut db = connection::open_db(&root.join(&space.db_file))?;
        run_migrations(&mut db, DbKind::Space.migrations())?;
        connection::integrity_check(&db)?;
    }
    Ok(())
}

/// Apply a prepared restore before layout creation or app connection initialization.  Any
/// failure moves the candidate aside and restores the exact old root before returning an error.
pub fn apply_pending_restore(root: &Path) -> VaultResult<bool> {
    let marker_path = marker_path(root)?;
    if !marker_path.exists() {
        return Ok(false);
    }
    let marker: PendingRestore = serde_json::from_slice(&fs::read(&marker_path)?)
        .map_err(|error| VaultError::Validation(format!("恢复标记无法解析: {error}")))?;
    let parent = parent_of(root)?;
    let staging = safe_staging(parent, &marker.staging_dir)?;
    if !staging.is_dir() || !root.is_dir() {
        return Err(VaultError::Validation(
            "恢复标记对应的数据目录不存在".into(),
        ));
    }
    let previous = parent.join(format!(
        "{}-pre-restore-{}",
        layout::APP_DIR_NAME,
        marker.nonce
    ));
    let failed = parent.join(format!(
        "{}-failed-restore-{}",
        layout::APP_DIR_NAME,
        marker.nonce
    ));
    fs::rename(root, &previous)?;
    let install = (|| -> VaultResult<()> {
        injected_failure("after_previous_rename")?;
        fs::rename(&staging, root)?;
        validate_after_install(root)
    })();
    match install {
        Ok(()) => {
            fs::remove_file(marker_path)?;
            let _ = fs::remove_file(diagnostic_path(root)?);
            Ok(true)
        }
        Err(error) => {
            let candidate = if root.exists() { root } else { &staging };
            if candidate.exists() {
                let _ = fs::rename(candidate, &failed);
            }
            let rollback = fs::rename(&previous, root);
            if let Err(rollback_error) = rollback {
                return Err(VaultError::Validation(format!(
                    "恢复失败 ({error}) 且无法回滚原数据: {rollback_error}"
                )));
            }
            let _ = fs::remove_file(marker_path);
            let diagnostic = RestoreDiagnostic {
                nonce: marker.nonce,
                failed_candidate: failed.display().to_string(),
                message: error.to_string(),
                occurred_at: chrono::Utc::now().to_rfc3339(),
            };
            let _ = write_diagnostic(root, &diagnostic);
            Err(VaultError::Validation(format!(
                "恢复失败，已回滚原数据；候选保留在 {}: {error}",
                failed.display()
            )))
        }
    }
}

#[cfg(test)]
thread_local! {
    static RESTORE_FAILURE_POINT: std::cell::RefCell<Option<&'static str>> = const { std::cell::RefCell::new(None) };
}

fn injected_failure(_point: &str) -> VaultResult<()> {
    #[cfg(test)]
    if RESTORE_FAILURE_POINT.with(|value| *value.borrow()) == Some(_point) {
        return Err(VaultError::Validation(format!(
            "测试注入恢复失败: {_point}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::writer;
    use crate::commands::AppState;
    use crate::db::startup;

    #[test]
    fn prepared_restore_keeps_live_root_until_startup_exchange() {
        let parent = tempfile::tempdir().unwrap();
        let root = layout::data_root(parent.path());
        let state = AppState::init(root.clone(), startup::startup(&root).unwrap()).unwrap();
        let archive = parent.path().join("backup.tvault");
        writer::create_backup(&state.inner, &archive, false, false).unwrap();
        let marker = prepare_restore(&root, &archive).unwrap();
        assert!(root.is_dir(), "preparation must not replace live data");
        assert!(parent.path().join(MARKER).is_file());
        drop(state);
        assert!(apply_pending_restore(&root).unwrap());
        assert!(root.is_dir());
        assert!(!parent.path().join(MARKER).exists());
        assert!(parent
            .path()
            .join(format!(
                "{}-pre-restore-{}",
                layout::APP_DIR_NAME,
                marker.nonce
            ))
            .is_dir());
    }

    #[test]
    fn injected_install_failures_restore_the_original_root_and_record_a_diagnostic() {
        for point in [
            "after_previous_rename",
            "during_migration",
            "during_integrity_check",
        ] {
            let parent = tempfile::tempdir().unwrap();
            let root = layout::data_root(parent.path());
            let state = AppState::init(root.clone(), startup::startup(&root).unwrap()).unwrap();
            let archive = parent.path().join("backup.tvault");
            writer::create_backup(&state.inner, &archive, false, false).unwrap();
            let marker = prepare_restore(&root, &archive).unwrap();
            drop(state);
            RESTORE_FAILURE_POINT.with(|failure| *failure.borrow_mut() = Some(point));
            let result = apply_pending_restore(&root);
            RESTORE_FAILURE_POINT.with(|failure| *failure.borrow_mut() = None);
            assert!(result.is_err(), "{point}");
            assert!(
                root.join(layout::META_DB).is_file(),
                "original data must be restored for {point}"
            );
            assert!(parent
                .path()
                .join(format!(
                    "{}-failed-restore-{}",
                    layout::APP_DIR_NAME,
                    marker.nonce
                ))
                .exists());
            assert!(read_diagnostic(&root)
                .unwrap()
                .unwrap()
                .message
                .contains(point));
        }
    }
}
