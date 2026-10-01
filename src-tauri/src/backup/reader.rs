//! Defensive `.tvault` reader shared by verification and restore preparation.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::backup::manifest::{BackupManifest, MAX_FILE_BYTES, MAX_TOTAL_BYTES};
use crate::db::connection;
use crate::db::layout;
use crate::db::migrate::{current_version, DbKind};
use crate::db::registry;
use crate::error::{VaultError, VaultResult};

fn bad(message: impl Into<String>) -> VaultError {
    VaultError::Validation(format!("备份包不安全或已损坏: {}", message.into()))
}

fn open_manifest(archive: &mut ZipArchive<File>) -> VaultResult<BackupManifest> {
    if archive.is_empty()
        || archive
            .by_index(0)
            .map_err(|error| bad(error.to_string()))?
            .name()
            != "manifest.json"
    {
        return Err(bad("manifest.json 必须是 ZIP 第一项"));
    }
    let entry = archive
        .by_name("manifest.json")
        .map_err(|_| bad("缺少 manifest.json"))?;
    if entry.size() > 4 * 1024 * 1024 || entry.is_dir() {
        return Err(bad("manifest.json 大小或类型非法"));
    }
    let manifest: BackupManifest = serde_json::from_reader(entry)
        .map_err(|error| bad(format!("manifest.json 无法解析: {error}")))?;
    manifest.validate()?;
    Ok(manifest)
}

/// Read only files declared by the validated manifest. This does not use generic extraction:
/// unknown entries, links, duplicate ZIP names and over-sized compressed entries are rejected.
pub fn extract_verified(archive_path: &Path, staging: &Path) -> VaultResult<BackupManifest> {
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file).map_err(|error| bad(error.to_string()))?;
    let manifest = open_manifest(&mut archive)?;
    let declared: HashSet<&str> = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    let mut seen = HashSet::new();
    if archive.len() != declared.len() + 1 {
        return Err(bad("ZIP 包含未声明或重复条目"));
    }
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| bad(error.to_string()))?;
        let name = entry.name().to_string();
        if !seen.insert(name.clone()) {
            return Err(bad(format!("ZIP 包含重复条目: {name}")));
        }
        if name == "manifest.json" {
            continue;
        }
        if !declared.contains(name.as_str()) || entry.is_dir() || entry.is_symlink() {
            return Err(bad(format!("ZIP 条目未声明或不是普通文件: {name}")));
        }
        if entry.size() > MAX_FILE_BYTES || entry.size() > MAX_TOTAL_BYTES {
            return Err(bad(format!("ZIP 条目超过安全大小: {name}")));
        }
    }
    fs::create_dir_all(staging)?;
    for declared_file in &manifest.files {
        let mut entry = archive
            .by_name(&declared_file.path)
            .map_err(|_| bad(format!("缺少清单声明文件: {}", declared_file.path)))?;
        if entry.size() != declared_file.size {
            return Err(bad(format!("文件大小与清单不符: {}", declared_file.path)));
        }
        let destination = staging.join(&declared_file.path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        let mut hasher = Sha256::new();
        let mut bytes = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = entry.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            if bytes > declared_file.size || bytes > MAX_FILE_BYTES {
                return Err(bad(format!("文件超过声明大小: {}", declared_file.path)));
            }
            output.write_all(&buffer[..count])?;
            hasher.update(&buffer[..count]);
        }
        if bytes != declared_file.size || format!("{:x}", hasher.finalize()) != declared_file.sha256
        {
            return Err(bad(format!("文件哈希与清单不符: {}", declared_file.path)));
        }
    }
    Ok(manifest)
}

/// Verify extracted databases and registry references before a restore can be prepared.
pub fn validate_staging(staging: &Path, manifest: &BackupManifest) -> VaultResult<()> {
    let check = |relative: &str, declared: u32, kind: DbKind| -> VaultResult<()> {
        let path = staging.join(relative);
        if !path.is_file() {
            return Err(bad(format!("缺少数据库: {relative}")));
        }
        let conn = connection::open_db(&path)
            .map_err(|error| bad(format!("数据库无法打开 {relative}: {error}")))?;
        connection::integrity_check(&conn)
            .map_err(|error| bad(format!("数据库完整性失败 {relative}: {error}")))?;
        let actual = current_version(&conn)? as u32;
        let supported = kind
            .migrations()
            .last()
            .map(|migration| migration.version as u32)
            .unwrap_or(0);
        if actual != declared || actual > supported {
            return Err(bad(format!("数据库 schema 不兼容 {relative}: {actual}")));
        }
        Ok(())
    };
    check(layout::META_DB, manifest.schema_versions.meta, DbKind::Meta)?;
    check(
        layout::TASKS_DB,
        manifest.schema_versions.tasks,
        DbKind::Tasks,
    )?;
    check(
        layout::CALENDAR_DB,
        manifest.schema_versions.calendar,
        DbKind::Calendar,
    )?;
    for space in &manifest.spaces {
        let version = manifest
            .schema_versions
            .spaces
            .iter()
            .find(|item| item.space_id == space.id)
            .ok_or_else(|| bad(format!("空间 schema 信息缺失: {}", space.id)))?;
        check(&space.db_file, version.version, DbKind::Space)?;
    }
    let meta = connection::open_db(&staging.join(layout::META_DB))?;
    let registry_ids: HashSet<String> = registry::list_spaces(&meta)?
        .into_iter()
        .map(|space| space.id)
        .collect();
    let manifest_ids: HashSet<String> = manifest
        .spaces
        .iter()
        .map(|space| space.id.clone())
        .collect();
    if registry_ids != manifest_ids {
        return Err(bad("空间注册表与清单不一致"));
    }
    let attachment_roots = std::iter::once((
        layout::TASKS_FILES_DIR.to_string(),
        staging.join(layout::TASKS_FILES_DIR),
    ))
    .chain(manifest.spaces.iter().map(|space| {
        let name = format!("{}/{}.files", layout::SPACES_DIR, space.id);
        (name.clone(), staging.join(name))
    }));
    for (relative, files_dir) in attachment_roots {
        let db_path = if relative == layout::TASKS_FILES_DIR {
            staging.join(layout::TASKS_DB)
        } else {
            let space_id = relative
                .trim_start_matches(&format!("{}/", layout::SPACES_DIR))
                .trim_end_matches(".files");
            let registration = manifest
                .spaces
                .iter()
                .find(|space| space.id == space_id)
                .unwrap();
            staging.join(&registration.db_file)
        };
        let conn = connection::open_db(&db_path)?;
        let mut statement = conn.prepare("SELECT hash FROM attachments WHERE hash IS NOT NULL")?;
        for hash in statement.query_map([], |row| row.get::<_, String>(0))? {
            let hash = hash?;
            if !files_dir.join(&hash).is_file() {
                return Err(bad(format!("附件引用缺少文件: {relative}/{hash}")));
            }
        }
    }
    Ok(())
}

/// Validate a package without mutating the active data root.
pub fn verify_archive(archive: &Path) -> VaultResult<BackupManifest> {
    let parent = archive.parent().ok_or_else(|| bad("备份路径缺少父目录"))?;
    let staging = parent.join(format!(".tvault-verify-{}", uuid::Uuid::new_v4()));
    let result = (|| -> VaultResult<BackupManifest> {
        let manifest = extract_verified(archive, &staging)?;
        validate_staging(&staging, &manifest)?;
        Ok(manifest)
    })();
    let _ = fs::remove_dir_all(staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::manifest::{ManifestFile, SchemaVersions, FORMAT_VERSION};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn rejects_unknown_zip_entry_before_extracting_anything() {
        let root = tempfile::tempdir().unwrap();
        let archive_path = root.path().join("malicious.tvault");
        let payload = b"ok";
        let manifest = BackupManifest {
            format_version: FORMAT_VERSION,
            app_version: "1.0.0".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            managed_auto_backup: false,
            schema_versions: SchemaVersions::default(),
            spaces: vec![],
            files: vec![ManifestFile {
                path: "safe.txt".into(),
                kind: "fixture".into(),
                size: payload.len() as u64,
                sha256: format!("{:x}", Sha256::digest(payload)),
            }],
        };
        let mut zip = zip::ZipWriter::new(File::create(&archive_path).unwrap());
        zip.start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        zip.start_file("safe.txt", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(payload).unwrap();
        zip.start_file("../escape.txt", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"no").unwrap();
        zip.finish().unwrap();
        let staging = root.path().join("staging");
        assert!(extract_verified(&archive_path, &staging).is_err());
        assert!(!root.path().join("escape.txt").exists());
        assert!(!staging.exists());
    }
}
