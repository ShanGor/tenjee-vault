//! `.tvault` manifest v1. The archive reader never trusts a ZIP entry before this model has
//! validated its declared relative path, size and content hash.

use std::collections::HashSet;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::error::{VaultError, VaultResult};

pub const FORMAT_VERSION: u32 = 1;
pub const MAX_FILES: usize = 10_000;
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupManifest {
    pub format_version: u32,
    pub app_version: String,
    pub created_at: String,
    #[serde(default)]
    pub managed_auto_backup: bool,
    pub schema_versions: SchemaVersions,
    pub spaces: Vec<SpaceRegistration>,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SchemaVersions {
    pub meta: u32,
    pub tasks: u32,
    pub calendar: u32,
    #[serde(default)]
    pub spaces: Vec<SpaceSchemaVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceSchemaVersion {
    pub space_id: String,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceRegistration {
    pub id: String,
    pub db_file: String,
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestFile {
    pub path: String,
    /// Examples: `database`, `space_database`, `space_attachment`, `task_attachment`.
    pub kind: String,
    pub size: u64,
    pub sha256: String,
}

impl BackupManifest {
    pub fn validate(&self) -> VaultResult<()> {
        if self.format_version > FORMAT_VERSION {
            return Err(VaultError::Validation(format!(
                "备份格式 v{} 比本应用支持的 v{} 更新",
                self.format_version, FORMAT_VERSION
            )));
        }
        if self.format_version == 0 || self.app_version.trim().is_empty() {
            return Err(VaultError::Validation(
                "备份清单缺少有效格式或应用版本".into(),
            ));
        }
        if self.files.len() > MAX_FILES {
            return Err(VaultError::Validation(
                "备份清单文件数量超过安全上限".into(),
            ));
        }
        let mut paths = HashSet::with_capacity(self.files.len());
        let mut total = 0_u64;
        for file in &self.files {
            validate_relative_path(&file.path)?;
            if !paths.insert(&file.path) {
                return Err(VaultError::Validation(format!(
                    "备份清单包含重复路径: {}",
                    file.path
                )));
            }
            if file.size > MAX_FILE_BYTES {
                return Err(VaultError::Validation(format!(
                    "备份文件过大: {}",
                    file.path
                )));
            }
            total = total
                .checked_add(file.size)
                .ok_or_else(|| VaultError::Validation("备份清单总大小溢出".into()))?;
            if total > MAX_TOTAL_BYTES {
                return Err(VaultError::Validation("备份清单总大小超过安全上限".into()));
            }
            if file.sha256.len() != 64 || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(VaultError::Validation(format!(
                    "备份文件 SHA-256 非法: {}",
                    file.path
                )));
            }
        }
        Ok(())
    }
}

pub fn validate_relative_path(value: &str) -> VaultResult<()> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(VaultError::Validation(format!("备份路径非法: {value}")));
    }
    if !path
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(VaultError::Validation(format!("备份路径非法: {value}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> BackupManifest {
        BackupManifest {
            format_version: FORMAT_VERSION,
            app_version: "1.0.0".into(),
            created_at: "2026-09-21T00:00:00Z".into(),
            managed_auto_backup: false,
            schema_versions: SchemaVersions::default(),
            spaces: vec![SpaceRegistration {
                id: "s1".into(),
                db_file: "spaces/s1.db".into(),
                encrypted: true,
            }],
            files: vec![ManifestFile {
                path: "databases/meta.db".into(),
                kind: "database".into(),
                size: 3,
                sha256: "a".repeat(64),
            }],
        }
    }

    #[test]
    fn manifest_round_trips_and_validates() {
        let manifest = manifest();
        let decoded: BackupManifest =
            serde_json::from_str(&serde_json::to_string(&manifest).unwrap()).unwrap();
        assert_eq!(decoded, manifest);
        decoded.validate().unwrap();
    }

    #[test]
    fn rejects_newer_duplicate_and_unsafe_paths() {
        let mut manifest = manifest();
        manifest.format_version = FORMAT_VERSION + 1;
        assert!(manifest.validate().is_err());
        manifest.format_version = FORMAT_VERSION;
        manifest.files.push(manifest.files[0].clone());
        assert!(manifest.validate().is_err());
        manifest.files.pop();
        for path in ["/absolute.db", "../escape.db"] {
            manifest.files[0].path = path.into();
            assert!(manifest.validate().is_err(), "{path} must be rejected");
        }
    }
}
