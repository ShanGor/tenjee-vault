//! Durable exchange files across Unix and Windows.
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

/// Windows FlushFileBuffers requires write access. File::open supplies only
/// read access, even when the file itself is writable.
pub(super) fn sync_file(path: &Path) -> io::Result<()> {
    OpenOptions::new().write(true).open(path)?.sync_all()
}

/// Publish the already-synchronized manifest before applying any staged data.
/// Unix persists the directory entry with fsync; Windows uses a write-through
/// rename rather than trying to open a directory as an ordinary file.
pub(super) fn publish_ready(directory: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

        // canonicalize supplies an absolute extended-length Windows path and
        // preserves Unicode, avoiding MAX_PATH limits for the native API.
        let directory = std::fs::canonicalize(directory)?;
        let from: Vec<u16> = directory
            .join("manifest.json")
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let to: Vec<u16> = directory
            .join("ready.json")
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: both buffers are NUL-terminated and live for the entire call.
        // No copy/delete or overwrite flags: the manifest stays on this volume
        // and an existing ready coordinator cannot be silently replaced.
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(
            directory.join("manifest.json"),
            directory.join("ready.json"),
        )?;
        std::fs::File::open(directory)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_blob_is_synchronized_without_changing_its_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let target = directory.path().join("copied");
        std::fs::write(&source, b"attachment ciphertext").unwrap();
        std::fs::copy(&source, &target).unwrap();
        sync_file(&target).unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"attachment ciphertext");
    }

    #[test]
    fn ready_manifest_is_published_with_unicode_paths() {
        let directory = tempfile::tempdir().unwrap();
        let stage = directory.path().join("交换-staging");
        std::fs::create_dir(&stage).unwrap();
        let manifest = stage.join("manifest.json");
        std::fs::write(&manifest, b"validated manifest").unwrap();
        sync_file(&manifest).unwrap();
        publish_ready(&stage).unwrap();
        assert!(!manifest.exists());
        assert_eq!(
            std::fs::read(stage.join("ready.json")).unwrap(),
            b"validated manifest"
        );
    }

    #[test]
    fn failed_publication_does_not_create_a_ready_manifest() {
        let directory = tempfile::tempdir().unwrap();
        assert!(publish_ready(directory.path()).is_err());
        assert!(!directory.path().join("ready.json").exists());
    }

    #[cfg(windows)]
    #[test]
    fn windows_regression_ordinary_directory_open_and_read_only_flush_fail() {
        let directory = tempfile::tempdir().unwrap();
        assert!(std::fs::File::open(directory.path()).is_err());
        let file = directory.path().join("blob");
        std::fs::write(&file, b"payload").unwrap();
        assert!(std::fs::File::open(&file).unwrap().sync_all().is_err());
        sync_file(&file).unwrap();
    }
}
