//! Atomic `.tvault` archive creation.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::backup::manifest::{
    BackupManifest, ManifestFile, SchemaVersions, SpaceRegistration, SpaceSchemaVersion,
    FORMAT_VERSION,
};
use crate::backup::FrozenPayload;
use crate::commands::AppStateInner;
use crate::db::connection;
use crate::db::migrate::current_version;
use crate::db::registry;
use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackupSummary {
    pub path: String,
    pub files: usize,
    pub total_bytes: u64,
    pub managed_auto_backup: bool,
}

fn zip_error(error: zip::result::ZipError) -> VaultError {
    VaultError::Validation(format!("无法创建备份 ZIP: {error}"))
}

fn digest(path: &Path) -> VaultResult<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let size = std::io::copy(&mut file, &mut hasher)?;
    Ok((format!("{:x}", hasher.finalize()), size))
}

fn archive_path(path: &Path) -> VaultResult<String> {
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| VaultError::Validation("备份路径不是有效 UTF-8".into()))
}

fn schemas_and_spaces(staging: &Path) -> VaultResult<(SchemaVersions, Vec<SpaceRegistration>)> {
    let meta = connection::open_db(&staging.join(crate::db::layout::META_DB))?;
    let mut spaces = registry::list_spaces(&meta)?;
    spaces.sort_by(|a, b| a.id.cmp(&b.id));
    let mut versions = SchemaVersions {
        meta: current_version(&meta)? as u32,
        tasks: current_version(&connection::open_db(
            &staging.join(crate::db::layout::TASKS_DB),
        )?)? as u32,
        calendar: current_version(&connection::open_db(
            &staging.join(crate::db::layout::CALENDAR_DB),
        )?)? as u32,
        spaces: Vec::new(),
    };
    let registrations = spaces
        .into_iter()
        .map(|space| {
            let db = connection::open_db(&staging.join(&space.db_file))?;
            let encrypted: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM sections WHERE is_encrypted = 1)",
                [],
                |row| row.get(0),
            )?;
            versions.spaces.push(SpaceSchemaVersion {
                space_id: space.id.clone(),
                version: current_version(&db)? as u32,
            });
            Ok(SpaceRegistration {
                id: space.id,
                db_file: space.db_file,
                encrypted,
            })
        })
        .collect::<VaultResult<Vec<_>>>()?;
    Ok((versions, registrations))
}

/// Build a backup package. `destination` must be the exact path selected by the user; callers
/// use the shared safe-destination boundary before reaching this domain service.
pub fn create_backup(
    inner: &AppStateInner,
    destination: &Path,
    overwrite: bool,
    managed_auto_backup: bool,
) -> VaultResult<BackupSummary> {
    if destination.extension().and_then(|value| value.to_str()) != Some("tvault") {
        return Err(VaultError::Validation(
            "备份文件必须使用 .tvault 扩展名".into(),
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| VaultError::Validation("备份目标缺少父目录".into()))?;
    if !parent.is_dir() {
        return Err(VaultError::Validation("备份目标目录不存在".into()));
    }
    if destination.exists() && !overwrite {
        return Err(VaultError::Validation("目标已存在；请明确确认覆盖".into()));
    }
    let staging = parent.join(format!(".tvault-staging-{}", uuid::Uuid::new_v4()));
    let temporary_archive = parent.join(format!(
        ".{}.{}.partial",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("backup.tvault"),
        uuid::Uuid::new_v4()
    ));
    let result = (|| -> VaultResult<BackupSummary> {
        fs::create_dir(&staging)?;
        let frozen = inner.snapshot_backup_payload(&staging)?;
        let frozen_by_path: HashMap<PathBuf, FrozenPayload> = frozen
            .into_iter()
            .map(|entry| (entry.relative_path.clone(), entry))
            .collect();
        let (schema_versions, spaces) = schemas_and_spaces(&staging)?;
        let mut files = Vec::with_capacity(frozen_by_path.len());
        for (relative, frozen) in &frozen_by_path {
            let (hash, size) = digest(&staging.join(relative))?;
            if hash != frozen.sha256 {
                return Err(VaultError::Validation(format!(
                    "压缩前备份内容发生变化: {}",
                    relative.display()
                )));
            }
            files.push(ManifestFile {
                path: archive_path(relative)?,
                kind: frozen.kind.clone(),
                size,
                sha256: hash,
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let total_bytes = files.iter().map(|file| file.size).sum();
        let manifest = BackupManifest {
            format_version: FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            managed_auto_backup,
            schema_versions,
            spaces,
            files,
        };
        manifest.validate()?;
        let archive = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_archive)?;
        let mut zip = ZipWriter::new(archive);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("manifest.json", options)
            .map_err(zip_error)?;
        zip.write_all(
            &serde_json::to_vec_pretty(&manifest)
                .map_err(|error| VaultError::Validation(format!("无法编码备份清单: {error}")))?,
        )?;
        for file in &manifest.files {
            zip.start_file(&file.path, options).map_err(zip_error)?;
            let mut source = File::open(staging.join(&file.path))?;
            std::io::copy(&mut source, &mut zip)?;
        }
        let archive = zip.finish().map_err(zip_error)?;
        archive.sync_all()?;
        drop(archive);
        if overwrite && destination.exists() {
            let replaced = parent.join(format!(
                ".{}.{}.replaced",
                destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("backup"),
                uuid::Uuid::new_v4()
            ));
            fs::rename(destination, &replaced)?;
            if let Err(error) = fs::rename(&temporary_archive, destination) {
                let _ = fs::rename(&replaced, destination);
                return Err(VaultError::Io(error));
            }
            fs::remove_file(replaced)?;
        } else {
            fs::rename(&temporary_archive, destination)?;
        }
        Ok(BackupSummary {
            path: destination.display().to_string(),
            files: manifest.files.len(),
            total_bytes,
            managed_auto_backup,
        })
    })();
    let _ = fs::remove_dir_all(&staging);
    if result.is_err() {
        let _ = fs::remove_file(&temporary_archive);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::AppState;
    use crate::db::{layout, startup};

    #[test]
    fn archive_contains_only_manifested_payloads_and_no_wal() {
        let parent = tempfile::tempdir().unwrap();
        let root = layout::data_root(parent.path());
        let report = startup::startup(&root).unwrap();
        let state = AppState::init(root, report).unwrap();
        let output = parent.path().join("safe.tvault");
        let summary = create_backup(&state.inner, &output, false, false).unwrap();
        assert!(summary.files >= 3);
        let mut archive = zip::ZipArchive::new(File::open(&output).unwrap()).unwrap();
        let manifest: BackupManifest =
            serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
        manifest.validate().unwrap();
        assert!(manifest
            .files
            .iter()
            .all(|file| !file.path.ends_with("-wal") && !file.path.ends_with("-shm")));
        assert_eq!(archive.len(), manifest.files.len() + 1);
        crate::backup::reader::verify_archive(&output).unwrap();
    }
}
