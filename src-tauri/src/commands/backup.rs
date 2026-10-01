//! Backup commands stay thin: dialog-selected paths are constrained before the archive service.

use serde::Serialize;
use std::path::Path;
use tauri::{Emitter, State};

use super::AppState;
use crate::backup::{reader, writer};
use crate::error::{VaultError, VaultResult};
use crate::portability::file_boundary::SafeDestination;

#[derive(Debug, Clone, Serialize)]
pub struct BackupVerification {
    pub files: usize,
    pub app_version: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreCleanup {
    pub removed: usize,
}

fn selected_backup_path(directory: &str, filename: &str) -> VaultResult<std::path::PathBuf> {
    let safe = SafeDestination::in_selected_directory(Path::new(directory), filename)?;
    if safe.path().extension().and_then(|value| value.to_str()) != Some("tvault") {
        return Err(VaultError::Validation(
            "备份文件必须使用 .tvault 扩展名".into(),
        ));
    }
    Ok(safe.path().to_path_buf())
}

#[tauri::command]
pub fn create_backup_cmd(
    state: State<'_, AppState>,
    directory: String,
    filename: String,
    overwrite: bool,
) -> Result<writer::BackupSummary, VaultError> {
    writer::create_backup(
        &state.inner,
        &selected_backup_path(&directory, &filename)?,
        overwrite,
        false,
    )
}

#[tauri::command]
pub fn verify_backup_cmd(
    directory: String,
    filename: String,
) -> Result<BackupVerification, VaultError> {
    let manifest = reader::verify_archive(&selected_backup_path(&directory, &filename)?)?;
    Ok(BackupVerification {
        files: manifest.files.len(),
        app_version: manifest.app_version,
        created_at: manifest.created_at,
    })
}

#[tauri::command]
pub fn prepare_restore_cmd(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    directory: String,
    filename: String,
) -> Result<String, VaultError> {
    let marker = crate::db::restore::prepare_restore(
        &state.inner.root,
        &selected_backup_path(&directory, &filename)?,
    )?;
    state.inner.session.lock_all();
    app.emit("restore-ready", &marker.nonce)
        .map_err(|error| VaultError::Validation(format!("无法通知恢复重启请求: {error}")))?;
    Ok(marker.nonce)
}

#[tauri::command]
pub fn get_restore_diagnostic_cmd(
    state: State<'_, AppState>,
) -> Result<Option<crate::db::restore::RestoreDiagnostic>, VaultError> {
    crate::db::restore::read_diagnostic(&state.inner.root)
}

#[tauri::command]
pub fn clear_pre_restore_copies_cmd(
    state: State<'_, AppState>,
) -> Result<RestoreCleanup, VaultError> {
    Ok(RestoreCleanup {
        removed: crate::db::restore::clear_pre_restore_copies(&state.inner.root)?,
    })
}
