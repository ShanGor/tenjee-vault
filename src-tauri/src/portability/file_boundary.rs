//! Safe destination handling for user-selected import/export paths.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use uuid::Uuid;

use crate::error::{VaultError, VaultResult};

/// A canonical user-selected directory plus a single safe output filename.
#[derive(Debug, Clone)]
pub struct SafeDestination {
    path: PathBuf,
}

impl SafeDestination {
    /// A caller must obtain `directory` from a user file dialog. This method rejects a nested,
    /// absolute, or traversal filename, so format adapters cannot write outside that selection.
    pub fn in_selected_directory(directory: &Path, filename: &str) -> VaultResult<Self> {
        let directory = fs::canonicalize(directory)
            .map_err(|error| VaultError::Validation(format!("无法规范化导出目录: {error}")))?;
        if !directory.is_dir() {
            return Err(VaultError::Validation("导出目标必须是目录".into()));
        }
        let candidate = Path::new(filename);
        if !matches!(candidate.components().next(), Some(Component::Normal(_)))
            || candidate.components().count() != 1
        {
            return Err(VaultError::Validation(
                "导出文件名必须是单个安全文件名".into(),
            ));
        }
        Ok(Self {
            path: directory.join(candidate),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write to a sibling temp file and atomically publish it. With `overwrite == false`, a
    /// hard-link publish prevents replacing a concurrent or pre-existing target.
    pub fn write_atomic(&self, bytes: &[u8], overwrite: bool) -> VaultResult<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| VaultError::Validation("导出目标缺少父目录".into()))?;
        let temp = parent.join(format!(
            ".{}.{}.partial",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("export"),
            Uuid::new_v4()
        ));
        let result = (|| -> VaultResult<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            if overwrite {
                fs::rename(&temp, &self.path)?;
            } else {
                fs::hard_link(&temp, &self.path).map_err(|error| match error.kind() {
                    std::io::ErrorKind::AlreadyExists => {
                        VaultError::Validation("目标已存在；请明确确认覆盖".into())
                    }
                    _ => VaultError::Io(error),
                })?;
                fs::remove_file(&temp)?;
            }
            let _ = File::open(parent).and_then(|directory| directory.sync_all());
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_preserves_existing_file_without_confirmation() {
        let root = tempfile::tempdir().unwrap();
        let destination = SafeDestination::in_selected_directory(root.path(), "export.md").unwrap();
        destination.write_atomic(b"old", false).unwrap();
        assert!(destination.write_atomic(b"new", false).is_err());
        assert_eq!(fs::read(destination.path()).unwrap(), b"old");
        assert!(SafeDestination::in_selected_directory(root.path(), "../escape.md").is_err());
    }

    #[test]
    fn confirmed_export_replaces_only_after_complete_temp_write() {
        let root = tempfile::tempdir().unwrap();
        let destination = SafeDestination::in_selected_directory(root.path(), "export.md").unwrap();
        destination.write_atomic(b"first", false).unwrap();
        destination.write_atomic(b"second", true).unwrap();
        assert_eq!(fs::read(destination.path()).unwrap(), b"second");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
