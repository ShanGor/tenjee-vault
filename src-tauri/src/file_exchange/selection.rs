use super::{destination, journal::Journal, manifest::*};
use crate::error::VaultResult;
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selected {
    pub handle: String,
    pub label: String,
}
pub fn prepare(
    journal: &Journal,
    sources: &[PathBuf],
    cancel: &AtomicBool,
) -> VaultResult<Preview> {
    let batch = journal.create("send", None, None, "")?;
    let work = (|| {
        let transaction = journal.conn.unchecked_transaction()?;
        let mut ordered = sources.to_vec();
        ordered.sort_by_key(|p| p.components().count());
        let mut accepted = Vec::<PathBuf>::new();
        let mut names = BTreeSet::new();
        let mut next = 0u32;
        for selected in ordered {
            if cancel.load(Ordering::SeqCst) {
                return Err(invalid("Preparation cancelled"));
            }
            if accepted.iter().any(|parent| selected.starts_with(parent)) {
                continue;
            }
            accepted.push(selected.clone());
            let original = selected
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| invalid("Selected root has an unsupported name"))?;
            component(original)?;
            let mut name = original.to_string();
            let mut n = 2;
            while !names.insert(key(&[name.clone()])) {
                name = format!("{original} ({n})");
                component(&name)?;
                n += 1;
            }
            let parent = destination::absolute_dir(
                selected
                    .parent()
                    .ok_or_else(|| invalid("Selected root has no parent"))?,
            )?;
            scan(
                journal,
                &batch,
                &parent,
                selected.file_name().unwrap(),
                &selected,
                vec![name],
                &mut next,
                cancel,
            )?;
        }
        journal.summary(&batch)?;
        journal.validate_tree(&batch)?;
        let digest = journal.digest(&batch)?;
        journal.set(&batch, "digest", &digest)?;
        transaction.commit()?;
        journal.summary(&batch)
    })();
    if work.is_err() {
        let _ = journal.delete(&batch);
    }
    work
}
#[allow(clippy::too_many_arguments)]
fn scan(
    j: &Journal,
    batch: &str,
    parent: &Dir,
    name: &std::ffi::OsStr,
    absolute: &Path,
    wire: Vec<String>,
    next: &mut u32,
    cancel: &AtomicBool,
) -> VaultResult<()> {
    if cancel.load(Ordering::SeqCst) {
        return Err(invalid("Preparation cancelled"));
    }
    if *next >= MAX_ENTRIES {
        return Err(invalid("Selection exceeds 100,000 entries"));
    }
    path(&wire)?;
    let add =
        |kind, size, reason: String, source: &str, sig: &str, next: &mut u32| -> VaultResult<()> {
            j.add(
                batch,
                &Entry {
                    id: *next,
                    path: wire.clone(),
                    kind,
                    size,
                    reason,
                },
                source,
                sig,
            )?;
            *next += 1;
            Ok(())
        };
    let metadata = match parent.symlink_metadata(name) {
        Ok(m) => m,
        Err(e) => {
            return add(
                Kind::Excluded,
                0,
                format!("Cannot read entry: {e}"),
                "",
                "",
                next,
            )
        }
    };
    if metadata.file_type().is_symlink() || destination::reject_reparse(&metadata).is_err() {
        return add(
            Kind::Excluded,
            0,
            "Symbolic links and reparse points are not transferred".into(),
            "",
            "",
            next,
        );
    }
    if metadata.is_file() {
        match parent
            .open_with(name, &destination::options(true, false, false))
            .and_then(|f| {
                destination::signature(&f).map_err(|e| std::io::Error::other(e.to_string()))
            }) {
            Ok(sig) => {
                let src = absolute
                    .to_str()
                    .ok_or_else(|| invalid("Non-UTF-8 source path is unsupported"))?;
                add(
                    Kind::File,
                    sig.size,
                    String::new(),
                    src,
                    &String::from_utf8(bytes(&sig)?).unwrap(),
                    next,
                )
            }
            Err(e) => add(
                Kind::Excluded,
                0,
                format!("Cannot open file: {e}"),
                "",
                "",
                next,
            ),
        }
    } else if metadata.is_dir() {
        let dir = match destination::open_dir(parent, name) {
            Ok(d) => d,
            Err(e) => {
                return add(
                    Kind::Excluded,
                    0,
                    format!("Cannot open folder: {e}"),
                    "",
                    "",
                    next,
                )
            }
        };
        add(Kind::Directory, 0, String::new(), "", "", next)?;
        let entries = match dir.entries() {
            Ok(e) => e,
            Err(e) => return Err(invalid(format!("Cannot enumerate selected folder: {e}"))),
        };
        // Native directory iterator bounds resident memory for large trees.
        for child in entries {
            let child = child?;
            let name = child.file_name();
            let Some(text) = name.to_str() else {
                return Err(invalid(
                    "Folder contains a non-UTF-8 name; remove that selection",
                ));
            };
            if let Err(e) = component(text) {
                return Err(invalid(format!(
                    "Folder contains an unrepresentable name; remove that selection: {e}"
                )));
            }
            let mut sub = wire.clone();
            sub.push(text.into());
            // Case/Unicode collisions cannot be silently sanitized or overwrite.
            scan(
                j,
                batch,
                &dir,
                &name,
                &absolute.join(&name),
                sub,
                next,
                cancel,
            )?;
        }
        Ok(())
    } else {
        add(
            Kind::Excluded,
            0,
            "Special files are not transferred".into(),
            "",
            "",
            next,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_selection_deduplicates_overlap_and_keeps_hidden_empty_and_duplicate_roots() {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("sources");
        std::fs::create_dir_all(source.join("a/folder/empty")).unwrap();
        std::fs::create_dir_all(source.join("b/folder")).unwrap();
        std::fs::write(source.join("a/folder/.hidden"), b"data").unwrap();
        std::fs::write(source.join("b/folder/file"), b"more").unwrap();
        let j = Journal::open(&t.path().join("journal")).unwrap();
        let p = prepare(
            &j,
            &[
                source.join("a/folder/.hidden"),
                source.join("a/folder"),
                source.join("b/folder"),
            ],
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(p.files, 2);
        assert_eq!(p.total_bytes, 8);
        let rows = j.page(&p.batch, 0, 100).unwrap();
        assert!(rows.iter().any(|e| e.path == ["folder", "empty"]));
        assert!(rows.iter().any(|e| e.path == ["folder (2)", "file"]));
    }
}
