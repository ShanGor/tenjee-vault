use super::manifest::{invalid, path, Entry};
use crate::error::VaultResult;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Identity {
    pub device: u64,
    pub inode: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Signature {
    pub identity: Identity,
    pub size: u64,
    pub modified: String,
}
pub fn identity(meta: &cap_std::fs::Metadata) -> Identity {
    Identity {
        device: meta.dev(),
        inode: meta.ino(),
    }
}
pub fn signature(file: &File) -> VaultResult<Signature> {
    let m = file.metadata()?;
    reject_reparse(&m)?;
    if !m.is_file() {
        return Err(invalid("Selected source is not a regular file"));
    }
    Ok(Signature {
        identity: identity(&m),
        size: m.len(),
        modified: format!("{:?}", m.modified()?),
    })
}
pub fn options(read: bool, write: bool, create: bool) -> OpenOptions {
    let mut o = OpenOptions::new();
    o.read(read)
        .write(write)
        .create_new(create)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NONBLOCK);
    }
    o
}

pub fn reject_reparse(meta: &cap_std::fs::Metadata) -> VaultResult<()> {
    #[cfg(windows)]
    if cap_std::fs::MetadataExt::file_attributes(meta) & 0x400 != 0 {
        return Err(invalid(
            "Windows reparse points are not transferred or traversed",
        ));
    }
    #[cfg(not(windows))]
    let _ = meta;
    Ok(())
}
pub fn open_dir(parent: &Dir, name: impl AsRef<Path>) -> VaultResult<Dir> {
    let dir = parent.open_dir_nofollow(name)?;
    reject_reparse(&dir.dir_metadata()?)?;
    Ok(dir)
}

// Pin every component before opening the next. A path traversal is never based
// on a previous canonicalize result followed by an unchecked ambient open.
pub fn absolute_dir(value: &Path) -> VaultResult<Dir> {
    if !value.is_absolute() {
        return Err(invalid("Selected directory must be absolute"));
    }
    let mut base = PathBuf::new();
    let mut names = Vec::new();
    for c in value.components() {
        match c {
            Component::Prefix(p) => base.push(p.as_os_str()),
            Component::RootDir => base.push(c.as_os_str()),
            Component::Normal(s) => names.push(s.to_owned()),
            _ => return Err(invalid("Invalid selected directory")),
        }
    }
    let mut dir = Dir::open_ambient_dir(base, cap_std::ambient_authority())?;
    for name in names {
        dir = open_dir(&dir, name)?;
    }
    Ok(dir)
}
pub fn source(value: &Path) -> VaultResult<File> {
    let parent = value
        .parent()
        .ok_or_else(|| invalid("Missing selected parent"))?;
    let name = value
        .file_name()
        .ok_or_else(|| invalid("Missing source name"))?;
    let file = absolute_dir(parent)?.open_with(name, &options(true, false, false))?;
    signature(&file)?;
    Ok(file)
}

pub struct Destination {
    pub base: Dir,
    pub output: Dir,
    pub staging: Dir,
    pub absolute: PathBuf,
    pub provider: Option<String>,
}
impl Destination {
    pub fn create(parent: &Path, name: &str) -> VaultResult<Self> {
        super::manifest::component(name)?;
        let base = absolute_dir(parent)?;
        base.create_dir(name)?;
        sync_directory(&base)?;
        let output = open_dir(&base, name)?;
        output.create_dir(".tenjee-partials")?;
        sync_directory(&output)?;
        let staging = open_dir(&output, ".tenjee-partials")?;
        // Check exclusive link publication capability before any content arrives.
        let probe = staging.open_with("probe", &options(false, true, true))?;
        probe.sync_all()?;
        #[cfg(not(windows))]
        {
            staging.hard_link("probe",&staging,"probe-link").map_err(|e|invalid(format!("Destination does not support safe file publication; choose another folder: {e}")))?;
            staging.remove_file("probe")?;
        }
        #[cfg(windows)]
        publish_windows(&staging, "probe", &staging, "probe-link")?;
        staging.remove_file("probe-link")?;
        sync_directory(&staging)?;
        Ok(Self {
            base,
            output,
            staging,
            absolute: parent.join(name),
            provider: None,
        })
    }
    pub fn provider_create(parent: &str, name: &str, batch: &str) -> VaultResult<Self> {
        let result = super::native::call(
            "createTransferBatch",
            serde_json::json!({"destination":parent,"name":name,"batch":batch}),
        )?;
        let private = PathBuf::from(
            result["privatePath"]
                .as_str()
                .ok_or_else(|| invalid("Invalid provider staging path"))?,
        );
        let uri = result["uri"]
            .as_str()
            .ok_or_else(|| invalid("Invalid provider batch URI"))?
            .to_string();
        let base = absolute_dir(
            private
                .parent()
                .ok_or_else(|| invalid("Invalid staging parent"))?,
        )?;
        let output = absolute_dir(&private)?;
        let staging = open_dir(&output, ".tenjee-partials")?;
        Ok(Self {
            base,
            output,
            staging,
            absolute: private,
            provider: Some(uri),
        })
    }
    pub fn name(&self) -> VaultResult<String> {
        if let Some(uri) = &self.provider {
            let result =
                super::native::call("restoreTransferBatch", serde_json::json!({"uri":uri}))?;
            return Ok(result["name"]
                .as_str()
                .ok_or_else(|| invalid("Provider batch name unavailable"))?
                .into());
        }
        Ok(self
            .absolute
            .file_name()
            .ok_or_else(|| invalid("Batch name unavailable"))?
            .to_string_lossy()
            .into_owned())
    }
    pub fn location(&self) -> String {
        self.provider
            .clone()
            .unwrap_or_else(|| self.absolute.to_string_lossy().into_owned())
    }
    pub fn reopen(absolute: &Path, expected: &Identity) -> VaultResult<Self> {
        if super::native::provider(&absolute.to_string_lossy()) {
            let result = super::native::call(
                "restoreTransferBatch",
                serde_json::json!({"uri":absolute.to_string_lossy()}),
            )?;
            let private = PathBuf::from(
                result["privatePath"]
                    .as_str()
                    .ok_or_else(|| invalid("Provider staging is unavailable"))?,
            );
            let mut d = Self::reopen(&private, expected)?;
            d.provider = Some(absolute.to_string_lossy().into_owned());
            return Ok(d);
        }
        let parent = absolute
            .parent()
            .ok_or_else(|| invalid("Invalid retained destination"))?;
        let base = absolute_dir(parent)?;
        let output = open_dir(
            &base,
            absolute
                .file_name()
                .ok_or_else(|| invalid("Invalid output directory"))?,
        )?;
        if identity(&output.dir_metadata()?) != *expected {
            return Err(invalid("Destination identity changed; choose a new batch"));
        }
        let staging = open_dir(&output, ".tenjee-partials")?;
        Ok(Self {
            base,
            output,
            staging,
            absolute: absolute.into(),
            provider: None,
        })
    }
    pub fn parent(&self, entry: &Entry, create: bool) -> VaultResult<(Dir, String)> {
        path(&entry.path)?;
        let mut dir = self.output.try_clone()?;
        for part in &entry.path[..entry.path.len() - 1] {
            if create {
                match dir.create_dir(part) {
                    Ok(()) => sync_directory(&dir)?,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e.into()),
                }
            }
            dir = open_dir(&dir, part)?;
        }
        Ok((dir, entry.path.last().unwrap().clone()))
    }
    pub fn directory(&self, entry: &Entry) -> VaultResult<()> {
        if let Some(uri) = &self.provider {
            super::native::call(
                "createTransferDirectory",
                serde_json::json!({"uri":uri,"path":entry.path}),
            )?;
        }
        let (parent, name) = self.parent(entry, true)?;
        match parent.create_dir(&name) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                open_dir(&parent, &name)?;
            }
            Err(e) => return Err(e.into()),
        };
        sync_directory(&parent)
    }
    pub fn verify_directory(&self, entry: &Entry) -> VaultResult<()> {
        if let Some(uri) = &self.provider {
            super::native::call(
                "verifyTransferDirectory",
                serde_json::json!({"uri":uri,"path":entry.path}),
            )?;
        }
        let (parent, name) = self.parent(entry, false)?;
        open_dir(&parent, name)?;
        Ok(())
    }
    pub fn partial(&self, id: u32) -> VaultResult<File> {
        let name = format!("{id}.part");
        let file = match self.staging.open_with(&name, &options(true, true, false)) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.staging.open_with(name, &options(true, true, true))?
            }
            Err(e) => return Err(e.into()),
        };
        signature(&file)?;
        Ok(file)
    }
    pub fn existing(&self, entry: &Entry) -> VaultResult<File> {
        if let Some(uri) = &self.provider {
            let result = super::native::call(
                "readSavedTransferEntry",
                serde_json::json!({"uri":uri,"id":entry.id,"path":entry.path,"size":entry.size.to_string()}),
            )?;
            if result["missing"] == true {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Provider output is not yet complete",
                )
                .into());
            }
            #[cfg(target_os = "android")]
            return super::native::descriptor(&result);
            #[cfg(not(target_os = "android"))]
            return Err(invalid("Document provider access requires Android"));
        }
        let (parent, name) = self.parent(entry, false)?;
        let file = parent.open_with(name, &options(true, false, false))?;
        signature(&file)?;
        Ok(file)
    }
    pub fn publish(&self, entry: &Entry) -> VaultResult<()> {
        self.publish_with(entry, |_| Ok(()))
    }
    pub fn publish_with(
        &self,
        entry: &Entry,
        mut boundary: impl FnMut(&str) -> VaultResult<()>,
    ) -> VaultResult<()> {
        if let Some(uri) = &self.provider {
            super::native::call(
                "publishTransferEntry",
                serde_json::json!({"uri":uri,"id":entry.id,"path":entry.path,"size":entry.size.to_string(),"source":self.absolute.join(".tenjee-partials").join(format!("{}.part",entry.id))}),
            )?;
            boundary("rename")?;
            boundary("directory-flush")?;
            self.discard(entry.id)?;
            return Ok(());
        }
        let (parent, name) = self.parent(entry, true)?;
        let partial = format!("{}.part", entry.id);
        #[cfg(windows)]
        {
            publish_windows(&self.staging, &partial, &parent, &name)?;
            boundary("rename")?;
            boundary("directory-flush")?;
            return Ok(());
        }
        #[cfg(not(windows))]
        {
            self.staging.hard_link(&partial, &parent, &name)?;
            boundary("rename")?;
            sync_directory(&parent)?;
            boundary("directory-flush")?;
            // Flush a writable handle: FlushFileBuffers on Windows rejects a
            // read-only one. No-replace hard link preserves concurrent user files.
            parent
                .open_with(&name, &options(false, true, false))?
                .sync_all()?;
            self.staging.remove_file(partial)?;
            sync_directory(&self.staging)?;
            Ok(())
        }
    }
    pub fn discard(&self, id: u32) -> VaultResult<()> {
        match self.staging.remove_file(format!("{id}.part")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
pub fn sync_directory(dir: &Dir) -> VaultResult<()> {
    // cap-std may pin a directory with O_PATH. Reopen "." as an ordinary
    // readable directory before fsync; fsync on an O_PATH handle returns EBADF.
    #[cfg(not(windows))]
    {
        use cap_fs_ext::OpenOptionsMaybeDirExt;
        let mut opts = options(true, false, false);
        opts.maybe_dir(true);
        dir.open_with(".", &opts)?.sync_all()?;
    }
    // Windows file publication uses MOVEFILE_WRITE_THROUGH below. Ordinary
    // directory FlushFileBuffers is unsupported; intent remains recoverable.
    #[cfg(windows)]
    let _ = dir;
    Ok(())
}

#[cfg(windows)]
fn publish_windows(from: &Dir, source: &str, to: &Dir, target: &str) -> VaultResult<()> {
    use std::os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    };
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, GetFinalPathNameByHandleW, MoveFileExW, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
            MOVEFILE_WRITE_THROUGH, OPEN_EXISTING,
        },
    };
    fn pinned(dir: &Dir) -> VaultResult<(std::fs::File, PathBuf)> {
        let file = dir.try_clone()?.into_std_file();
        let mut path = vec![0u16; 32768];
        let len = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle() as _,
                path.as_mut_ptr(),
                path.len() as u32,
                0,
            )
        };
        if len == 0 || len as usize >= path.len() {
            return Err(std::io::Error::last_os_error().into());
        }
        path.truncate(len as usize);
        let value = PathBuf::from(std::ffi::OsString::from_wide(&path));
        path.push(0);
        // Excluding FILE_SHARE_DELETE pins this directory's pathname through
        // the write-through move. Reparse points are not followed by this open.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        let lock = unsafe { std::fs::File::from_raw_handle(handle as _) };
        let actual = cap_std::fs::File::from_std(lock.try_clone()?);
        if identity(&actual.metadata()?) != identity(&dir.dir_metadata()?) {
            return Err(invalid("Destination changed during publication"));
        }
        Ok((lock, value))
    }
    use std::os::windows::ffi::OsStringExt;
    let (_source_lock, source_parent) = pinned(from)?;
    let (_target_lock, target_parent) = pinned(to)?;
    let source: Vec<u16> = source_parent
        .join(source)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let target: Vec<u16> = target_parent
        .join(target)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // No REPLACE_EXISTING or COPY_ALLOWED: atomic, same-volume, no overwrite.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::manifest::Kind;
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn publication_never_replaces_existing_file() {
        let t = tempfile::tempdir().unwrap();
        let d = Destination::create(t.path(), "batch").unwrap();
        let e = Entry {
            id: 0,
            path: vec!["nested".into(), "文件.txt".into()],
            kind: Kind::File,
            size: 3,
            reason: String::new(),
        };
        d.partial(0).unwrap().write_all(b"abc").unwrap();
        d.publish(&e).unwrap();
        d.partial(0).unwrap().write_all(b"xyz").unwrap();
        assert!(d.publish(&e).is_err());
        let mut b = Vec::new();
        d.existing(&e).unwrap().read_to_end(&mut b).unwrap();
        assert_eq!(b, b"abc");
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape_even_after_batch_creation() {
        let t = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let d = Destination::create(t.path(), "batch").unwrap();
        std::os::unix::fs::symlink(outside.path(), d.absolute.join("evil")).unwrap();
        let e = Entry {
            id: 0,
            path: vec!["evil".into(), "file".into()],
            kind: Kind::File,
            size: 0,
            reason: String::new(),
        };
        assert!(d.parent(&e, true).is_err());
        assert!(!outside.path().join("file").exists());
    }
    #[cfg(windows)]
    #[test]
    fn windows_reparse_directories_cannot_escape_destination() {
        let t = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let d = Destination::create(t.path(), "batch").unwrap();
        std::os::windows::fs::symlink_dir(outside.path(), d.absolute.join("evil")).unwrap();
        let meta = std::fs::symlink_metadata(d.absolute.join("evil")).unwrap();
        assert!(
            meta.file_type().is_symlink(),
            "OS did not create a reparse point: {meta:?}"
        );
        let entry = Entry {
            id: 0,
            path: vec!["evil".into(), "file".into()],
            kind: Kind::File,
            size: 0,
            reason: String::new(),
        };
        assert!(d.parent(&entry, true).is_err());
        assert!(!outside.path().join("file").exists());
    }
}
