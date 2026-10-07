//! Notes-file import service. Parsing happens before page creation so invalid inputs never leave
//! a visible half-page; encrypted targets are checked before any filesystem or database write.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

use super::document::{Block, PortableDocument, Resource};
use super::text::BatchResult;
use super::{html, markdown, text};
use crate::error::{VaultError, VaultResult};
use crate::notes::session::SessionManager;
use crate::notes::{attachments, hierarchy, pages};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteImportFormat {
    Markdown,
    Html,
    Text,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportedPage {
    pub page_id: String,
    pub title: String,
    pub attachment_count: usize,
}

pub fn import_files(
    conn: &mut Connection,
    files_dir: &Path,
    session: &SessionManager,
    section_id: &str,
    paths: &[PathBuf],
) -> VaultResult<BatchResult<ImportedPage>> {
    // This is intentionally before parsing: a locked target receives no page, attachment, or
    // observable partial batch write.
    if crate::notes::section_encrypted(conn, section_id)? {
        session.with_dsk(section_id, |_| Ok(()))?;
    }
    let mut result = BatchResult::default();
    for path in paths {
        let label = path.display().to_string();
        match import_file(conn, files_dir, session, section_id, path) {
            Ok(page) => result.success(label, page),
            Err(error) => result.failed(label, error.to_string()),
        }
    }
    Ok(result)
}

fn import_file(
    conn: &mut Connection,
    files_dir: &Path,
    session: &SessionManager,
    section_id: &str,
    path: &Path,
) -> VaultResult<ImportedPage> {
    let bytes = std::fs::read(path)?;
    let format = format_for(path)?;
    let mut document = match format {
        NoteImportFormat::Markdown => markdown::parse(
            std::str::from_utf8(&bytes)
                .map_err(|_| VaultError::Validation("Markdown 必须是 UTF-8".into()))?,
        )?,
        NoteImportFormat::Html => html::parse(
            std::str::from_utf8(&bytes)
                .map_err(|_| VaultError::Validation("HTML 必须是 UTF-8".into()))?,
        )?,
        NoteImportFormat::Text => text::parse(&bytes)?,
    };
    let source_root = path
        .parent()
        .ok_or_else(|| VaultError::Validation("导入文件缺少父目录".into()))?;
    let resources = read_local_resources(&document, source_root)?;
    let title = path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("导入页面")
        .to_owned();
    let encrypted=crate::notes::section_encrypted(conn,section_id)?;
    if encrypted { session.with_dsk(section_id, |_| Ok(()))?; }
    let page = hierarchy::create_page_prepared(conn, section_id, None, &title,false)?;
    let imported = (|| -> VaultResult<usize> {
        for (source, name, bytes) in &resources {
            let attachment = attachments::save_attachment(
                files_dir, conn, session, section_id, "page", &page.id, name, None, bytes,
            )?;
            replace_resource_source(
                &mut document.blocks,
                source,
                &format!("attachment://{}", attachment.id),
            );
        }
        let content = document.to_tiptap_json().to_string();
        pages::save_page(conn, session, &page.id, &title, &content)?;
        Ok(resources.len())
    })();
    match imported {
        Ok(attachment_count) => Ok(ImportedPage {
            page_id: page.id,
            title,
            attachment_count,
        }),
        Err(error) => {
            let _ = attachments::purge_attachments_of_page(files_dir, conn, &page.id);
            let _ = hierarchy::delete_page(conn, &page.id);
            Err(error)
        }
    }
}

fn format_for(path: &Path) -> VaultResult<NoteImportFormat> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("md" | "markdown") => Ok(NoteImportFormat::Markdown),
        Some("html" | "htm") => Ok(NoteImportFormat::Html),
        Some("txt") => Ok(NoteImportFormat::Text),
        _ => Err(VaultError::Validation(
            "仅支持 Markdown、HTML 和纯文本文件".into(),
        )),
    }
}

fn read_local_resources(
    document: &PortableDocument,
    root: &Path,
) -> VaultResult<Vec<(String, String, Vec<u8>)>> {
    markdown::collect_resources(document)
        .into_iter()
        .map(|reference| {
            let source = Path::new(&reference.source);
            if source.is_absolute()
                || source
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
            {
                return Err(VaultError::Validation(
                    "导入资源路径不能越出导入文件目录".into(),
                ));
            }
            let path = root.join(source);
            let bytes = std::fs::read(&path).map_err(|error| {
                VaultError::Validation(format!("无法读取资源 {}: {error}", path.display()))
            })?;
            Ok((reference.source, reference.relative_path, bytes))
        })
        .collect()
}

fn replace_resource_source(blocks: &mut [Block], from: &str, to: &str) {
    for block in blocks {
        match block {
            Block::Image { resource } | Block::Attachment { resource } => {
                replace(resource, from, to)
            }
            Block::List { items, .. } => {
                for item in items {
                    replace_resource_source(&mut item.blocks, from, to);
                }
            }
            Block::TaskList { items } => {
                for item in items {
                    replace_resource_source(&mut item.blocks, from, to);
                }
            }
            Block::Table { rows } => {
                for row in rows {
                    for cell in &mut row.cells {
                        replace_resource_source(&mut cell.blocks, from, to);
                    }
                }
            }
            _ => {}
        }
    }
}
fn replace(resource: &mut Resource, from: &str, to: &str) {
    if resource.source == from {
        resource.source = to.to_owned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::configure;
    use crate::db::migrate::{run_migrations, DbKind};
    use crate::notes::hierarchy;

    #[test]
    fn imports_valid_text_and_reports_invalid_files_without_pages() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("hello.txt");
        std::fs::write(&input, "hello\nworld").unwrap();
        let bad = root.path().join("bad.bin");
        std::fs::write(&bad, "x").unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        let notebook = hierarchy::create_notebook(&conn, "n", None).unwrap();
        let section = hierarchy::create_section(&conn, &notebook.id, None, "s", None).unwrap();
        let session = SessionManager::new();
        let files = root.path().join("files");
        std::fs::create_dir(&files).unwrap();
        let result = import_files(&mut conn, &files, &session, &section.id, &[input, bad]).unwrap();
        assert_eq!((result.succeeded(), result.failed_count()), (1, 1));
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pages WHERE is_deleted = 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
