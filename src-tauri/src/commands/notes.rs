//! 笔记相关 commands：层级 CRUD、页面读写、版本历史、回收站、最近使用、附件、搜索。

use rusqlite::OptionalExtension;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

use super::{AppState, AppStateInner};
use crate::error::{VaultError, VaultResult};
use crate::notes::{attachments, hierarchy, pages};
use crate::portability::{
    export::{self, NoteExportFormat},
    file_boundary::SafeDestination,
    notes as note_import,
};
use crate::search::SearchHit;

fn need(inner: &AppStateInner, space_id: &str) -> VaultResult<()> {
    let _ = inner;
    if space_id.is_empty() {
        return Err(VaultError::Validation("缺少空间 id".into()));
    }
    Ok(())
}

// ---------------------------------------------------------------- 导航树

#[derive(Debug, Clone, Serialize)]
pub struct TreeDto {
    pub notebooks: Vec<hierarchy::NotebookNode>,
    /// 当前解锁的加密分区（前端据此渲染锁定态）
    pub unlocked_section_ids: Vec<String>,
}

#[tauri::command]
pub fn get_tree(state: State<'_, AppState>, space_id: String) -> Result<TreeDto, VaultError> {
    need(&state.inner, &space_id)?;
    let tree = state
        .inner
        .with_space(&space_id, |conn| hierarchy::notebook_tree(conn))?;
    Ok(TreeDto {
        notebooks: tree,
        unlocked_section_ids: state.inner.session.unlocked_section_ids(),
    })
}

#[tauri::command]
pub fn get_page_tree(
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
) -> Result<Vec<hierarchy::PageNode>, VaultError> {
    state
        .inner
        .with_space(&space_id, |conn| hierarchy::page_tree(conn, &section_id))
}

// ---------------------------------------------------------------- 笔记本

#[tauri::command]
pub fn create_notebook_cmd(
    state: State<'_, AppState>,
    space_id: String,
    name: String,
    color: Option<String>,
) -> Result<hierarchy::Notebook, VaultError> {
    if name.trim().is_empty() {
        return Err(VaultError::Validation("笔记本名称不能为空".into()));
    }
    state.inner.with_space(&space_id, |conn| {
        hierarchy::create_notebook(conn, name.trim(), color.as_deref())
    })
}

#[tauri::command]
pub fn rename_notebook_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
    name: String,
) -> Result<(), VaultError> {
    if name.trim().is_empty() {
        return Err(VaultError::Validation("笔记本名称不能为空".into()));
    }
    state.inner.with_space(&space_id, |conn| {
        hierarchy::rename_notebook(conn, &id, name.trim())
    })
}

#[tauri::command]
pub fn set_notebook_color_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
    color: Option<String>,
) -> Result<(), VaultError> {
    state.inner.with_space(&space_id, |conn| {
        hierarchy::set_notebook_color(conn, &id, color.as_deref())
    })
}

#[tauri::command]
pub fn reorder_notebooks_cmd(
    state: State<'_, AppState>,
    space_id: String,
    ids: Vec<String>,
) -> Result<(), VaultError> {
    state
        .inner
        .with_space(&space_id, |conn| hierarchy::reorder_notebooks(conn, &ids))
}

#[tauri::command]
pub fn delete_notebook_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
) -> Result<(), VaultError> {
    state
        .inner
        .with_space(&space_id, |conn| hierarchy::delete_notebook(conn, &id))
}

// ---------------------------------------------------------------- 分区组

#[tauri::command]
pub fn create_section_group_cmd(
    state: State<'_, AppState>,
    space_id: String,
    notebook_id: String,
    parent_group_id: Option<String>,
    name: String,
) -> Result<hierarchy::SectionGroup, VaultError> {
    if name.trim().is_empty() {
        return Err(VaultError::Validation("分区组名称不能为空".into()));
    }
    state.inner.with_space(&space_id, |conn| {
        hierarchy::create_section_group(conn, &notebook_id, parent_group_id.as_deref(), name.trim())
    })
}

#[tauri::command]
pub fn rename_section_group_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
    name: String,
) -> Result<(), VaultError> {
    if name.trim().is_empty() {
        return Err(VaultError::Validation("分区组名称不能为空".into()));
    }
    state.inner.with_space(&space_id, |conn| {
        hierarchy::rename_section_group(conn, &id, name.trim())
    })
}

#[tauri::command]
pub fn move_section_group_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
    notebook_id: String,
    parent_group_id: Option<String>,
) -> Result<(), VaultError> {
    state.inner.with_space(&space_id, |conn| {
        hierarchy::move_section_group(conn, &id, &notebook_id, parent_group_id.as_deref())
    })
}

#[tauri::command]
pub fn delete_section_group_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
) -> Result<(), VaultError> {
    state
        .inner
        .with_space(&space_id, |conn| hierarchy::delete_section_group(conn, &id))
}

// ---------------------------------------------------------------- 分区

#[tauri::command]
pub fn create_section_cmd(
    state: State<'_, AppState>,
    space_id: String,
    notebook_id: String,
    section_group_id: Option<String>,
    name: String,
    color: Option<String>,
) -> Result<hierarchy::Section, VaultError> {
    if name.trim().is_empty() {
        return Err(VaultError::Validation("分区名称不能为空".into()));
    }
    state.inner.with_space(&space_id, |conn| {
        hierarchy::create_section(
            conn,
            &notebook_id,
            section_group_id.as_deref(),
            name.trim(),
            color.as_deref(),
        )
    })
}

#[tauri::command]
pub fn rename_section_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
    name: String,
) -> Result<(), VaultError> {
    if name.trim().is_empty() {
        return Err(VaultError::Validation("分区名称不能为空".into()));
    }
    state.inner.with_space(&space_id, |conn| {
        hierarchy::rename_section(conn, &id, name.trim())
    })
}

#[tauri::command]
pub fn set_section_color_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
    color: Option<String>,
) -> Result<(), VaultError> {
    state.inner.with_space(&space_id, |conn| {
        hierarchy::set_section_color(conn, &id, color.as_deref())
    })
}

#[tauri::command]
pub fn move_section_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
    notebook_id: String,
    section_group_id: Option<String>,
) -> Result<(), VaultError> {
    state.inner.with_space(&space_id, |conn| {
        hierarchy::move_section(conn, &id, &notebook_id, section_group_id.as_deref())
    })
}

#[tauri::command]
pub fn reorder_sections_cmd(
    state: State<'_, AppState>,
    space_id: String,
    ids: Vec<String>,
) -> Result<(), VaultError> {
    state
        .inner
        .with_space(&space_id, |conn| hierarchy::reorder_sections(conn, &ids))
}

#[tauri::command]
pub fn delete_section_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
) -> Result<(), VaultError> {
    state
        .inner
        .with_space(&space_id, |conn| hierarchy::delete_section(conn, &id))
}

// ---------------------------------------------------------------- 页面

#[tauri::command]
pub fn create_page_cmd(
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
    parent_page_id: Option<String>,
    title: String,
) -> Result<hierarchy::PageSummary, VaultError> {
    state.inner.with_space(&space_id, |conn| {
        hierarchy::create_page(conn, &section_id, parent_page_id.as_deref(), title.trim())
    })
}

#[tauri::command]
pub fn rename_page_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
    title: String,
) -> Result<(), VaultError> {
    state.inner.with_space(&space_id, |conn| {
        hierarchy::rename_page(conn, &id, title.trim())
    })
}

#[tauri::command]
pub fn move_page_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
    section_id: String,
    parent_page_id: Option<String>,
    sort_order: i64,
) -> Result<(), VaultError> {
    state.inner.with_space(&space_id, |conn| {
        hierarchy::move_page(
            conn,
            &id,
            &section_id,
            parent_page_id.as_deref(),
            sort_order,
        )
    })
}

#[tauri::command]
pub fn delete_page_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
) -> Result<(), VaultError> {
    state
        .inner
        .with_space(&space_id, |conn| hierarchy::delete_page(conn, &id))
}

#[tauri::command]
pub fn restore_page_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
) -> Result<(), VaultError> {
    state
        .inner
        .with_space(&space_id, |conn| hierarchy::restore_page(conn, &id))
}

#[tauri::command]
pub fn purge_page_cmd(
    state: State<'_, AppState>,
    space_id: String,
    id: String,
) -> Result<(), VaultError> {
    let files = state.inner.files_dir(&space_id);
    state.inner.with_space(&space_id, |conn| {
        attachments::purge_attachments_of_page(&files, conn, &id)?;
        hierarchy::purge_page(conn, &id)
    })
}

#[tauri::command]
pub fn list_trash_cmd(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<Vec<serde_json::Value>, VaultError> {
    state
        .inner
        .with_space(&space_id, |conn| hierarchy::list_trash(conn))
}

#[tauri::command]
pub fn get_page_cmd(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<pages::Page, VaultError> {
    super::note_tasks::sync(&state.inner)?;
    let session = &state.inner.session;
    state
        .inner
        .with_space(&space_id, |conn| pages::get_page(conn, session, &page_id))
}

#[tauri::command]
pub fn save_page_cmd(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
    title: String,
    content: String,
) -> Result<(), VaultError> {
    let session = &state.inner.session;
    state.inner.with_space(&space_id, |conn| {
        pages::save_page(conn, session, &page_id, &title, &content)
    })?;
    super::note_tasks::from_note(&state.inner,&space_id,&page_id,&content)
}

#[tauri::command]
pub fn list_versions_cmd(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<Vec<pages::PageVersion>, VaultError> {
    let session = &state.inner.session;
    state.inner.with_space(&space_id, |conn| {
        pages::list_versions(conn, session, &page_id)
    })
}

#[tauri::command]
pub fn rollback_version_cmd(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
    version_id: String,
) -> Result<(), VaultError> {
    let session = &state.inner.session;
    state.inner.with_space(&space_id, |conn| {
        pages::rollback_version(conn, session, &page_id, &version_id)
    })
}

// ---------------------------------------------------------------- 导入 / 导出

fn export_destination(directory: &str, filename: &str) -> VaultResult<String> {
    Ok(
        SafeDestination::in_selected_directory(Path::new(directory), filename)?
            .path()
            .to_string_lossy()
            .to_string(),
    )
}

fn selected_input_paths(paths: Vec<String>) -> VaultResult<Vec<PathBuf>> {
    if paths.is_empty() {
        return Err(VaultError::Validation("请至少选择一个导入文件".into()));
    }
    paths
        .into_iter()
        .map(PathBuf::from)
        .map(|path| {
            if !path.is_absolute() || !path.is_file() {
                return Err(VaultError::Validation(
                    "导入路径必须是文件对话框选择的绝对常规文件".into(),
                ));
            }
            Ok(path)
        })
        .collect()
}

/// Creates a short-lived confirmation capability only after the UI has displayed its plaintext
/// export warning. It is bound to one source and destination and can be used once.
#[tauri::command]
pub fn request_page_export_confirmation_cmd(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
    format: String,
    directory: String,
    filename: String,
) -> Result<String, VaultError> {
    let format = NoteExportFormat::parse(&format)?;
    let destination = export_destination(&directory, &filename)?;
    let page = state.inner.with_space(&space_id, |conn| {
        pages::get_page(conn, &state.inner.session, &page_id)
    })?;
    let encrypted = state.inner.with_space(&space_id, |conn| {
        crate::notes::section_encrypted(conn, &page.section_id)
    })?;
    if !encrypted {
        return Err(VaultError::Validation("普通分区导出不需要明文确认".into()));
    }
    state.inner.issue_export_confirmation(
        format!("page:{page_id}"),
        destination,
        format.key().into(),
    )
}

#[tauri::command]
pub fn request_section_export_confirmation_cmd(
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
    format: String,
    directory: String,
    name: String,
) -> Result<String, VaultError> {
    let format = NoteExportFormat::parse(&format)?;
    let destination = export_destination(&directory, &name)?;
    let encrypted = state.inner.with_space(&space_id, |conn| {
        if crate::notes::section_encrypted(conn, &section_id)? {
            state.inner.session.with_dsk(&section_id, |_| Ok(()))?;
        }
        crate::notes::section_encrypted(conn, &section_id)
    })?;
    if !encrypted {
        return Err(VaultError::Validation("普通分区导出不需要明文确认".into()));
    }
    state.inner.issue_export_confirmation(
        format!("section:{section_id}"),
        destination,
        format.key().into(),
    )
}

#[tauri::command]
pub fn import_note_files_cmd(
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
    paths: Vec<String>,
) -> Result<crate::portability::text::BatchResult<note_import::ImportedPage>, VaultError> {
    let paths = selected_input_paths(paths)?;
    let files = state.inner.files_dir(&space_id);
    state.inner.with_space(&space_id, |conn| {
        note_import::import_files(conn, &files, &state.inner.session, &section_id, &paths)
    })
}

#[tauri::command]
pub fn export_page_cmd(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
    format: String,
    directory: String,
    filename: String,
    overwrite: bool,
    confirmation_token: Option<String>,
) -> Result<(), VaultError> {
    let format = NoteExportFormat::parse(&format)?;
    let destination = export_destination(&directory, &filename)?;
    let encrypted = state.inner.with_space(&space_id, |conn| {
        let page = pages::get_page(conn, &state.inner.session, &page_id)?;
        crate::notes::section_encrypted(conn, &page.section_id)
    })?;
    if encrypted {
        let token = confirmation_token
            .as_deref()
            .ok_or_else(|| VaultError::Validation("导出加密页面前必须确认明文输出".into()))?;
        state.inner.consume_export_confirmation(
            token,
            &format!("page:{page_id}"),
            &destination,
            format.key(),
        )?;
    }
    let files = state.inner.files_dir(&space_id);
    state.inner.with_space(&space_id, |conn| {
        export::export_page(
            conn,
            &files,
            &state.inner.session,
            &page_id,
            format,
            Path::new(&directory),
            &filename,
            overwrite,
        )
    })
}

#[tauri::command]
pub fn export_section_cmd(
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
    format: String,
    directory: String,
    name: String,
    overwrite: bool,
    confirmation_token: Option<String>,
) -> Result<(), VaultError> {
    let format = NoteExportFormat::parse(&format)?;
    let destination = export_destination(&directory, &name)?;
    let encrypted = state.inner.with_space(&space_id, |conn| {
        crate::notes::section_encrypted(conn, &section_id)
    })?;
    if encrypted {
        let token = confirmation_token
            .as_deref()
            .ok_or_else(|| VaultError::Validation("导出加密分区前必须确认明文输出".into()))?;
        state.inner.consume_export_confirmation(
            token,
            &format!("section:{section_id}"),
            &destination,
            format.key(),
        )?;
    }
    let files = state.inner.files_dir(&space_id);
    state.inner.with_space(&space_id, |conn| {
        export::export_section(
            conn,
            &files,
            &state.inner.session,
            &section_id,
            format,
            Path::new(&directory),
            &name,
            overwrite,
        )
    })
}

// ---------------------------------------------------------------- 最近使用

#[tauri::command]
pub fn record_page_open_cmd(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<(), VaultError> {
    let meta = state
        .inner
        .meta
        .lock()
        .map_err(|_| VaultError::Validation("主库锁中毒".into()))?;
    meta.execute(
        "INSERT INTO recent_pages (space_id, page_id, opened_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(space_id, page_id) DO UPDATE SET opened_at = excluded.opened_at",
        rusqlite::params![space_id, page_id],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentEntry {
    pub space_id: String,
    pub page_id: String,
    pub title: String,
    pub opened_at: String,
    /// 分区已加密且未解锁（前端隐藏或标记不可打开）
    pub locked: bool,
}

#[tauri::command]
pub fn list_recent_cmd(
    state: State<'_, AppState>,
    limit: u32,
) -> Result<Vec<RecentEntry>, VaultError> {
    let rows: Vec<(String, String, String)> = {
        let meta = state
            .inner
            .meta
            .lock()
            .map_err(|_| VaultError::Validation("主库锁中毒".into()))?;
        let mut stmt = meta.prepare(
            "SELECT space_id, page_id, opened_at FROM recent_pages ORDER BY opened_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![limit.max(1) as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    let mut out = Vec::new();
    for (space_id, page_id, opened_at) in rows {
        // 标题与锁定态：页面可能被删或所在分区加密未解锁——此时跳过（spec: 最近列表排除锁定分区）
        let lookup = state.inner.with_space(&space_id, |conn| {
            let found: Option<(String, i64)> = conn
                .query_row(
                    "SELECT s.id, s.is_encrypted FROM pages p JOIN sections s ON s.id = p.section_id WHERE p.id = ?1 AND p.is_deleted = 0",
                    rusqlite::params![page_id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
                )
                .optional()?;
            Ok(found)
        });
        let (section_id, encrypted) = match lookup {
            Ok(Some(v)) => v,
            _ => continue,
        };
        let locked = encrypted != 0 && !state.inner.session.is_unlocked(&section_id);
        if locked {
            continue;
        }
        let title = state.inner.with_space(&space_id, |conn| {
            let t: String = conn.query_row(
                "SELECT title FROM pages WHERE id = ?1",
                rusqlite::params![page_id],
                |r| r.get(0),
            )?;
            Ok(t)
        });
        let Ok(title) = title else { continue };
        out.push(RecentEntry {
            space_id,
            page_id,
            title,
            opened_at,
            locked: false,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- 附件

#[tauri::command]
pub fn save_attachment_cmd(
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
    entity_id: String,
    file_name: String,
    mime: Option<String>,
    data_base64: String,
) -> Result<attachments::Attachment, VaultError> {
    let bytes = crate::notes::base64_decode(&data_base64)?;
    let files = state.inner.files_dir(&space_id);
    let session = &state.inner.session;
    state.inner.with_space(&space_id, |conn| {
        attachments::save_attachment(
            &files,
            conn,
            session,
            &section_id,
            "page",
            &entity_id,
            &file_name,
            mime.as_deref(),
            &bytes,
        )
    })
}

#[tauri::command]
pub fn open_attachment_cmd(
    state: State<'_, AppState>,
    space_id: String,
    attachment_id: String,
) -> Result<attachments::AttachmentData, VaultError> {
    let files = state.inner.files_dir(&space_id);
    let session = &state.inner.session;
    state.inner.with_space(&space_id, |conn| {
        attachments::open_attachment(&files, conn, session, &attachment_id)
    })
}

#[tauri::command]
pub fn delete_attachment_cmd(
    state: State<'_, AppState>,
    space_id: String,
    attachment_id: String,
) -> Result<(), VaultError> {
    let files = state.inner.files_dir(&space_id);
    state.inner.with_space(&space_id, |conn| {
        attachments::delete_attachment(&files, conn, &attachment_id)
    })
}

#[tauri::command]
pub fn list_attachments_cmd(
    state: State<'_, AppState>,
    space_id: String,
    entity_id: String,
) -> Result<Vec<attachments::Attachment>, VaultError> {
    state.inner.with_space(&space_id, |conn| {
        attachments::list_attachments(conn, "page", &entity_id)
    })
}

// ---------------------------------------------------------------- 页面标题列表（[[ 双链选择）

#[derive(Debug, Clone, Serialize)]
pub struct PageTitle {
    pub id: String,
    pub title: String,
    pub section_id: String,
}

/// 全空间页面标题（锁定加密分区排除，spec: 锁定可见性）。
#[tauri::command]
pub fn list_page_titles_cmd(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<Vec<PageTitle>, VaultError> {
    let unlocked = state.inner.session.unlocked_section_ids();
    state.inner.with_space(&space_id, |conn| {
        let mut stmt = conn.prepare(
            "SELECT p.id, p.title, p.section_id FROM pages p
             JOIN sections s ON s.id = p.section_id
             WHERE p.is_deleted = 0 AND (s.is_encrypted = 0 OR s.id IN (
                 SELECT value FROM json_each(?1)
             ))",
        )?;
        let unlocked_json = serde_json::to_string(&unlocked).unwrap_or_else(|_| "[]".to_string());
        let rows = stmt
            .query_map(rusqlite::params![unlocked_json], |r| {
                Ok(PageTitle {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    section_id: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

// ---------------------------------------------------------------- 搜索

#[derive(Debug, Clone, Serialize)]
pub struct SearchResultDto {
    pub page_id: String,
    pub section_id: String,
    pub section_name: String,
    pub notebook_name: String,
    pub title: String,
    pub snippet: String,
    /// 命中是否来自解锁的加密分区
    pub from_unlocked: bool,
}

#[tauri::command]
pub fn search_notes_cmd(
    state: State<'_, AppState>,
    space_id: String,
    query: String,
    notebook_id: Option<String>,
    section_id: Option<String>,
    limit: u32,
) -> Result<Vec<SearchResultDto>, VaultError> {
    let session = &state.inner.session;
    let hits: Vec<SearchHit> = state.inner.with_space(&space_id, |conn| {
        let unlocked = session.unlocked_section_ids();
        crate::search::search(conn, &query, &unlocked, limit.max(1) as usize)
    })?;
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    // 上下文（分区 → 笔记本）映射
    let context: Vec<(String, String, String)> = state.inner.with_space(&space_id, |conn| {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, n.name FROM sections s JOIN notebooks n ON n.id = s.notebook_id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    let unlocked_ids = state.inner.session.unlocked_section_ids();
    let mut out = Vec::new();
    for hit in hits {
        let Some((_, section_name, notebook_name)) =
            context.iter().find(|(sid, _, _)| *sid == hit.section_id)
        else {
            continue;
        };
        if let Some(nb) = &notebook_id {
            let nb_id: Option<String> = state.inner.with_space(&space_id, |conn| {
                Ok(conn
                    .query_row(
                        "SELECT notebook_id FROM sections WHERE id = ?1",
                        rusqlite::params![hit.section_id],
                        |r| r.get(0),
                    )
                    .optional()?)
            })?;
            if nb_id.as_ref() != Some(nb) {
                continue;
            }
        }
        if let Some(sec) = &section_id {
            if &hit.section_id != sec {
                continue;
            }
        }
        out.push(SearchResultDto {
            page_id: hit.page_id,
            section_id: hit.section_id.clone(),
            section_name: section_name.clone(),
            notebook_name: notebook_name.clone(),
            title: hit.title,
            snippet: hit.snippet,
            from_unlocked: unlocked_ids.contains(&hit.section_id),
        });
    }
    Ok(out)
}
