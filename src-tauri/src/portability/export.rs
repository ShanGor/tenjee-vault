//! Page export service. It uses the portable AST so every output format shares encryption,
//! resource, and safe-destination behavior.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use rusqlite::Connection;

use super::document::{Block, Resource};
use super::file_boundary::SafeDestination;
use super::{html, markdown, pdf};
use crate::error::{VaultError, VaultResult};
use crate::notes::session::SessionManager;
use crate::notes::{attachments, hierarchy, pages};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteExportFormat {
    Markdown,
    Html,
    Pdf,
}

impl NoteExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Pdf => "pdf",
        }
    }

    pub fn parse(value: &str) -> VaultResult<Self> {
        match value {
            "markdown" | "md" => Ok(Self::Markdown),
            "html" => Ok(Self::Html),
            "pdf" => Ok(Self::Pdf),
            _ => Err(VaultError::Validation(
                "仅支持 Markdown、HTML 或 PDF 导出".into(),
            )),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Pdf => "pdf",
        }
    }
}

/// Export a single page after checking its current session. A locked encrypted page fails before
/// the destination is touched. Callers supply the dialog-selected directory and filename.
pub fn export_page(
    conn: &Connection,
    files_dir: &Path,
    session: &SessionManager,
    page_id: &str,
    format: NoteExportFormat,
    directory: &Path,
    filename: &str,
    overwrite: bool,
) -> VaultResult<()> {
    let destination = SafeDestination::in_selected_directory(directory, filename)?;
    write_page_export(
        conn,
        files_dir,
        session,
        page_id,
        format,
        destination.path(),
        overwrite,
    )
}

/// Export every live page in a section into a single, atomically-published directory.  Page
/// filenames include a stable id suffix so duplicate titles cannot collide; child pages are
/// placed below a directory named after their parent.  Links using TipTap's `page://<id>` form
/// are rewritten to relative exported paths when the destination contains that page.
pub fn export_section(
    conn: &Connection,
    files_dir: &Path,
    session: &SessionManager,
    section_id: &str,
    format: NoteExportFormat,
    directory: &Path,
    name: &str,
    overwrite: bool,
) -> VaultResult<()> {
    let destination = SafeDestination::in_selected_directory(directory, name)?;
    if destination.path().exists() && !overwrite {
        return Err(VaultError::Validation(
            "导出目录已存在；请明确确认覆盖".into(),
        ));
    }
    // Do this before creating staging.  It also establishes the no-write behavior for a locked
    // encrypted target; every page read below repeats the check in case it locks mid-export.
    if crate::notes::section_encrypted(conn, section_id)? {
        session.with_dsk(section_id, |_| Ok(()))?;
    }
    let mut tree = hierarchy::page_tree(conn, section_id)?;
    crate::notes::page_tree::hydrate_legacy(conn, session, &mut tree)?;
    export_tree(
        conn, files_dir, session, &tree, format, directory, name, overwrite,
    )
}

pub fn export_subtree(
    conn: &Connection,
    files_dir: &Path,
    session: &SessionManager,
    page_id: &str,
    format: NoteExportFormat,
    directory: &Path,
    name: &str,
    overwrite: bool,
) -> VaultResult<()> {
    fn convert(node: crate::notes::page_tree::Node) -> hierarchy::PageNode {
        hierarchy::PageNode {
            page: node.page,
            children: node.children.into_iter().map(convert).collect(),
        }
    }
    fn find(
        nodes: Vec<crate::notes::page_tree::Node>,
        id: &str,
    ) -> Option<crate::notes::page_tree::Node> {
        for node in nodes {
            if node.page.id == id {
                return Some(node);
            }
            if let Some(found) = find(node.children, id) {
                return Some(found);
            }
        }
        None
    }
    let mut tree = crate::notes::page_tree::tree(conn)?;
    crate::notes::page_tree::hydrate(conn, session, &mut tree)?;
    let root = find(tree, page_id).ok_or_else(|| VaultError::NotFound("Page".into()))?;
    for id in crate::notes::page_tree::descendants(conn, page_id)? {
        let live: bool =
            conn.query_row("SELECT is_deleted=0 FROM pages WHERE id=?1", [&id], |r| {
                r.get(0)
            })?;
        if live {
            pages::get_page(conn, session, &id)?;
        }
    }
    export_tree(
        conn,
        files_dir,
        session,
        &[convert(root)],
        format,
        directory,
        name,
        overwrite,
    )
}

fn export_tree(
    conn: &Connection,
    files_dir: &Path,
    session: &SessionManager,
    tree: &[hierarchy::PageNode],
    format: NoteExportFormat,
    directory: &Path,
    name: &str,
    overwrite: bool,
) -> VaultResult<()> {
    let destination = SafeDestination::in_selected_directory(directory, name)?;
    let parent = destination
        .path()
        .parent()
        .ok_or_else(|| VaultError::Validation("导出目标缺少父目录".into()))?;
    let staging = parent.join(format!(".{}.{}.partial", name, uuid::Uuid::new_v4()));
    let result = (|| -> VaultResult<()> {
        std::fs::create_dir(&staging)?;
        let mut paths = HashMap::new();
        plan_page_paths(&tree, Path::new(""), format.extension(), &mut paths)?;
        for (page_id, relative) in &paths {
            let output = staging.join(relative);
            write_section_page(conn, files_dir, session, page_id, format, &output, &paths)?;
        }
        publish_directory(&staging, destination.path(), overwrite)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn write_page_export(
    conn: &Connection,
    files_dir: &Path,
    session: &SessionManager,
    page_id: &str,
    format: NoteExportFormat,
    output: &Path,
    overwrite: bool,
) -> VaultResult<()> {
    let page = pages::get_page(conn, session, page_id)?;
    let tiptap = serde_json::from_str(if page.content.is_empty() {
        r#"{"type":"doc","content":[{"type":"paragraph"}]}"#
    } else {
        &page.content
    })
    .map_err(|error| VaultError::Validation(format!("页面内容不是有效 TipTap JSON: {error}")))?;
    let mut document = super::document::PortableDocument::from_tiptap_json(&tiptap)?;
    let assets = publish_resources(
        conn,
        files_dir,
        session,
        &mut document.blocks,
        output,
        overwrite,
    )?;
    let bytes = match format {
        NoteExportFormat::Markdown => markdown::render(&document).into_bytes(),
        NoteExportFormat::Html => html::render(&document).into_bytes(),
        NoteExportFormat::Pdf => {
            pdf::render_with_resource_root(&document, &page.title, assets.as_deref())?
        }
    };
    let write = SafeDestination::in_selected_directory(
        output
            .parent()
            .ok_or_else(|| VaultError::Validation("导出目标缺少父目录".into()))?,
        output
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| VaultError::Validation("导出文件名无效".into()))?,
    )?
    .write_atomic(&bytes, overwrite);
    if write.is_err() {
        if let Some(assets) = assets {
            let _ = std::fs::remove_dir_all(assets);
        }
    }
    write
}

fn write_section_page(
    conn: &Connection,
    files_dir: &Path,
    session: &SessionManager,
    page_id: &str,
    format: NoteExportFormat,
    output: &Path,
    paths: &HashMap<String, PathBuf>,
) -> VaultResult<()> {
    let page = pages::get_page(conn, session, page_id)?;
    let tiptap = serde_json::from_str(if page.content.is_empty() {
        r#"{"type":"doc","content":[{"type":"paragraph"}]}"#
    } else {
        &page.content
    })
    .map_err(|error| VaultError::Validation(format!("页面内容不是有效 TipTap JSON: {error}")))?;
    let mut document = super::document::PortableDocument::from_tiptap_json(&tiptap)?;
    rewrite_page_links(&mut document.blocks, output, paths)?;
    let parent = output
        .parent()
        .ok_or_else(|| VaultError::Validation("导出目标缺少父目录".into()))?;
    std::fs::create_dir_all(parent)?;
    let assets = publish_resources(
        conn,
        files_dir,
        session,
        &mut document.blocks,
        output,
        false,
    )?;
    let bytes = match format {
        NoteExportFormat::Markdown => markdown::render(&document).into_bytes(),
        NoteExportFormat::Html => html::render(&document).into_bytes(),
        NoteExportFormat::Pdf => {
            pdf::render_with_resource_root(&document, &page.title, assets.as_deref())?
        }
    };
    SafeDestination::in_selected_directory(
        parent,
        output
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| VaultError::Validation("导出文件名无效".into()))?,
    )?
    .write_atomic(&bytes, false)
}

fn plan_page_paths(
    nodes: &[hierarchy::PageNode],
    base: &Path,
    extension: &str,
    out: &mut HashMap<String, PathBuf>,
) -> VaultResult<()> {
    for node in nodes {
        let stem = safe_page_stem(&node.page.title, &node.page.id);
        let file = base.join(format!("{stem}.{extension}"));
        out.insert(node.page.id.clone(), file);
        plan_page_paths(&node.children, &base.join(stem), extension, out)?;
    }
    Ok(())
}

fn safe_page_stem(title: &str, id: &str) -> String {
    let slug: String = title
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            {
                '-'
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim_matches(|ch: char| ch == '.' || ch.is_whitespace())
        .chars()
        .take(80)
        .collect();
    let slug = if slug.is_empty() { "page" } else { &slug };
    format!("{slug}-{}", &id[..id.len().min(8)])
}

fn rewrite_page_links(
    blocks: &mut [Block],
    output: &Path,
    paths: &HashMap<String, PathBuf>,
) -> VaultResult<()> {
    let from = output
        .parent()
        .ok_or_else(|| VaultError::Validation("导出目标缺少父目录".into()))?;
    for block in blocks {
        match block {
            Block::Blockquote { blocks } => rewrite_page_links(blocks, output, paths)?,
            Block::Paragraph { content } | Block::Heading { content, .. } => {
                rewrite_inlines(content, from, paths)
            }
            Block::List { items, .. } => {
                for item in items {
                    rewrite_page_links(&mut item.blocks, output, paths)?;
                }
            }
            Block::TaskList { items } => {
                for item in items {
                    rewrite_page_links(&mut item.blocks, output, paths)?;
                }
            }
            Block::Table { rows } => {
                for row in rows {
                    for cell in &mut row.cells {
                        rewrite_page_links(&mut cell.blocks, output, paths)?;
                    }
                }
            }
            Block::Image { .. }
            | Block::Attachment { .. }
            | Block::CodeBlock { .. }
            | Block::HorizontalRule => {}
        }
    }
    Ok(())
}

fn rewrite_inlines(
    inlines: &mut [super::document::Inline],
    from: &Path,
    paths: &HashMap<String, PathBuf>,
) {
    for inline in inlines {
        if let super::document::Inline::Link { href, .. } = inline {
            if let Some(id) = href.strip_prefix("page://") {
                if let Some(target) = paths.get(id) {
                    if let Some(relative) = relative_path(from, target) {
                        *href = relative.to_string_lossy().replace('\\', "/");
                    }
                }
            }
        }
    }
}

fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
    let from = from.components().collect::<Vec<_>>();
    let to = to.components().collect::<Vec<_>>();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut result = PathBuf::new();
    for _ in common..from.len() {
        result.push("..");
    }
    for component in &to[common..] {
        if let Component::Normal(value) = component {
            result.push(value);
        } else {
            return None;
        }
    }
    Some(result)
}

fn publish_directory(staging: &Path, target: &Path, overwrite: bool) -> VaultResult<()> {
    if target.exists() && !overwrite {
        return Err(VaultError::Validation(
            "导出目录已存在；请明确确认覆盖".into(),
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| VaultError::Validation("导出目标缺少父目录".into()))?;
    let backup = parent.join(format!(
        ".{}.{}.backup",
        target
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("export"),
        uuid::Uuid::new_v4()
    ));
    if target.exists() {
        std::fs::rename(target, &backup)?;
    }
    if let Err(error) = std::fs::rename(staging, target) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, target);
        }
        return Err(error.into());
    }
    if backup.exists() {
        std::fs::remove_dir_all(backup)?;
    }
    Ok(())
}

/// Resolve an attachment at export time. The open path rechecks the encryption session, so a
/// section locked mid-export returns `SectionLocked` and the caller can discard staging output.
pub fn export_attachment_bytes(
    conn: &Connection,
    files_dir: &Path,
    session: &SessionManager,
    attachment_id: &str,
) -> VaultResult<Vec<u8>> {
    let data = attachments::open_attachment(files_dir, conn, session, attachment_id)?;
    crate::notes::base64_decode(&data.data_base64)
}

fn publish_resources(
    conn: &Connection,
    files_dir: &Path,
    session: &SessionManager,
    blocks: &mut [Block],
    document_path: &Path,
    overwrite: bool,
) -> VaultResult<Option<PathBuf>> {
    let has_resources = blocks.iter().any(block_has_resource);
    if !has_resources {
        return Ok(None);
    }
    let parent = document_path
        .parent()
        .ok_or_else(|| VaultError::Validation("导出目标缺少父目录".into()))?;
    let stem = document_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("export");
    let final_dir = parent.join(format!("{stem}_assets"));
    if final_dir.exists() && !overwrite {
        return Err(VaultError::Validation(
            "资源目录已存在；请明确确认覆盖".into(),
        ));
    }
    let staging = parent.join(format!(".{stem}_assets.{}.partial", uuid::Uuid::new_v4()));
    std::fs::create_dir(&staging)?;
    let backup = parent.join(format!(".{stem}_assets.{}.backup", uuid::Uuid::new_v4()));
    let resource_names = markdown::collect_resources(&super::document::PortableDocument {
        blocks: blocks.to_vec(),
    })
    .into_iter()
    .map(|reference| (reference.source, reference.relative_path))
    .collect::<HashMap<_, _>>();
    let result = (|| -> VaultResult<()> {
        rewrite_resources(
            conn,
            files_dir,
            session,
            blocks,
            &staging,
            &format!("{stem}_assets"),
            &resource_names,
        )?;
        if final_dir.exists() {
            std::fs::rename(&final_dir, &backup)?;
        }
        if let Err(error) = std::fs::rename(&staging, &final_dir) {
            if backup.exists() {
                let _ = std::fs::rename(&backup, &final_dir);
            }
            return Err(error.into());
        }
        if backup.exists() {
            std::fs::remove_dir_all(&backup)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result.map(|_| Some(final_dir))
}

fn block_has_resource(block: &Block) -> bool {
    match block {
        Block::Blockquote { blocks } => blocks.iter().any(block_has_resource),
        Block::Image { .. } | Block::Attachment { .. } => true,
        Block::List { items, .. } => items
            .iter()
            .any(|item| item.blocks.iter().any(block_has_resource)),
        Block::TaskList { items } => items
            .iter()
            .any(|item| item.blocks.iter().any(block_has_resource)),
        Block::Table { rows } => rows.iter().any(|row| {
            row.cells
                .iter()
                .any(|cell| cell.blocks.iter().any(block_has_resource))
        }),
        _ => false,
    }
}
fn rewrite_resources(
    conn: &Connection,
    files_dir: &Path,
    session: &SessionManager,
    blocks: &mut [Block],
    directory: &Path,
    relative_dir: &str,
    resource_names: &HashMap<String, String>,
) -> VaultResult<()> {
    for block in blocks {
        match block {
            Block::Blockquote { blocks } => rewrite_resources(
                conn,
                files_dir,
                session,
                blocks,
                directory,
                relative_dir,
                resource_names,
            )?,
            Block::Image { resource } | Block::Attachment { resource } => copy_resource(
                conn,
                files_dir,
                session,
                resource,
                directory,
                relative_dir,
                resource_names,
            )?,
            Block::List { items, .. } => {
                for item in items {
                    rewrite_resources(
                        conn,
                        files_dir,
                        session,
                        &mut item.blocks,
                        directory,
                        relative_dir,
                        resource_names,
                    )?;
                }
            }
            Block::TaskList { items } => {
                for item in items {
                    rewrite_resources(
                        conn,
                        files_dir,
                        session,
                        &mut item.blocks,
                        directory,
                        relative_dir,
                        resource_names,
                    )?;
                }
            }
            Block::Table { rows } => {
                for row in rows {
                    for cell in &mut row.cells {
                        rewrite_resources(
                            conn,
                            files_dir,
                            session,
                            &mut cell.blocks,
                            directory,
                            relative_dir,
                            resource_names,
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
fn copy_resource(
    conn: &Connection,
    files_dir: &Path,
    session: &SessionManager,
    resource: &mut Resource,
    directory: &Path,
    relative_dir: &str,
    resource_names: &HashMap<String, String>,
) -> VaultResult<()> {
    let id = resource.source.trim_start_matches("attachment://");
    if id == resource.source {
        return Ok(());
    }
    let data = export_attachment_bytes(conn, files_dir, session, id)?;
    let name = resource_names
        .get(&resource.source)
        .cloned()
        .unwrap_or_else(|| format!("attachment-{id}"));
    let path = directory.join(&name);
    if !path.exists() {
        std::fs::write(path, data)?;
    }
    resource.source = format!("{relative_dir}/{name}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::configure;
    use crate::db::migrate::{run_migrations, DbKind};
    use crate::notes::hierarchy;

    #[test]
    fn exports_page_only_to_selected_safe_destination() {
        let root = tempfile::tempdir().unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        let notebook = hierarchy::create_notebook(&conn, "n", None).unwrap();
        let section = hierarchy::create_section(&conn, &notebook.id, None, "s", None).unwrap();
        let page = hierarchy::create_page(&conn, &section.id, None, "p").unwrap();
        let session = SessionManager::new();
        pages::save_page(&mut conn, &session, &page.id, "标题", r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"内容"}]}]}"#).unwrap();
        export_page(
            &conn,
            root.path(),
            &session,
            &page.id,
            NoteExportFormat::Markdown,
            root.path(),
            "page.md",
            false,
        )
        .unwrap();
        assert!(std::fs::read_to_string(root.path().join("page.md"))
            .unwrap()
            .contains("内容"));
        assert!(export_page(
            &conn,
            root.path(),
            &session,
            &page.id,
            NoteExportFormat::Html,
            root.path(),
            "../bad.html",
            false
        )
        .is_err());
    }

    #[test]
    fn exports_section_hierarchy_and_rewrites_internal_page_links() {
        let root = tempfile::tempdir().unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        let notebook = hierarchy::create_notebook(&conn, "n", None).unwrap();
        let section = hierarchy::create_section(&conn, &notebook.id, None, "s", None).unwrap();
        let parent = hierarchy::create_page(&conn, &section.id, None, "Parent").unwrap();
        let child = hierarchy::create_page(&conn, &section.id, Some(&parent.id), "Child").unwrap();
        let session = SessionManager::new();
        let source = serde_json::json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"pageLink","attrs":{"pageId":child.id,"label":"Child"}}]}]}).to_string();
        pages::save_page(&mut conn, &session, &parent.id, "Parent", &source).unwrap();
        pages::save_page(&mut conn, &session, &child.id, "Child", r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"nested"}]}]}"#).unwrap();

        export_section(
            &conn,
            root.path(),
            &session,
            &section.id,
            NoteExportFormat::Markdown,
            root.path(),
            "section",
            false,
        )
        .unwrap();
        let exported = root.path().join("section");
        let parent_file = std::fs::read_dir(&exported)
            .unwrap()
            .find_map(|entry| {
                let path = entry.unwrap().path();
                path.extension()
                    .is_some_and(|extension| extension == "md")
                    .then_some(path)
            })
            .unwrap();
        let child_dir = exported.join(parent_file.file_stem().unwrap());
        let child_file = std::fs::read_dir(&child_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let parent_markdown = std::fs::read_to_string(parent_file).unwrap();
        assert!(!parent_markdown.contains("page://"));
        assert!(parent_markdown.contains(&*child_file.file_name().unwrap().to_string_lossy()));
        assert!(std::fs::read_to_string(child_file)
            .unwrap()
            .contains("nested"));
        assert!(export_section(
            &conn,
            root.path(),
            &session,
            &section.id,
            NoteExportFormat::Markdown,
            root.path(),
            "section",
            false
        )
        .is_err());
    }
}
