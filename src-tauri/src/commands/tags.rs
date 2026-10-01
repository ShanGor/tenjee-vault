//! Commands that coordinate the meta-owned tag dictionary with local domain links.

use serde::{Deserialize, Serialize};
use tauri::State;

use super::AppState;
use crate::{
    db::registry,
    error::VaultError,
    tags::{self, Tag},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaggedKind {
    Note,
    Task,
    Calendar,
}

#[derive(Debug, Serialize)]
pub struct TaggedItem {
    pub kind: TaggedKind,
    pub id: String,
    pub title: String,
    pub route: String,
    pub status: Option<String>,
    pub archived: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct TagCounts {
    pub note: usize,
    pub task: usize,
    pub calendar: usize,
}

#[derive(Debug, Serialize)]
pub struct TagResults {
    pub tag: Option<Tag>,
    pub items: Vec<TaggedItem>,
    pub counts: TagCounts,
    pub unavailable_spaces: Vec<String>,
}

/// Counts are derived only from accessible entities, before applying the type filter.
/// Missing dictionary entries remain usable so stale associations can be removed.
pub fn aggregate_tag(
    inner: &super::AppStateInner,
    tag_id: &str,
    kind: Option<TaggedKind>,
    include_archived: bool,
) -> Result<TagResults, VaultError> {
    let (tag, spaces) = inner.with_meta(|conn| {
        Ok((
            tags::list(conn)?.into_iter().find(|tag| tag.id == tag_id),
            registry::list_spaces(conn)?,
        ))
    })?;
    let mut items = inner.with_tasks(|conn| {
        let mut statement = conn.prepare(
            "SELECT id, title, status, archived_at IS NOT NULL FROM tasks t
             WHERE (?2 OR archived_at IS NULL) AND EXISTS
             (SELECT 1 FROM taggings g WHERE g.entity_type = 'task' AND g.entity_id = t.id AND g.tag_id = ?1)
             ORDER BY id")?;
        let rows = statement.query_map(rusqlite::params![tag_id, include_archived], |row| {
            let id: String = row.get(0)?;
            Ok(TaggedItem { kind: TaggedKind::Task, route: format!("#/tasks?task={id}"),
                id, title: row.get(1)?, status: row.get(2)?, archived: row.get(3)? })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    items.extend(inner.with_calendar(|conn| {
        let mut statement = conn.prepare(
            "SELECT id, title FROM events e WHERE EXISTS
             (SELECT 1 FROM taggings g WHERE g.entity_type = 'event' AND g.entity_id = e.id AND g.tag_id = ?1)
             ORDER BY id")?;
        let rows = statement.query_map([tag_id], |row| {
            let id: String = row.get(0)?;
            Ok(TaggedItem { kind: TaggedKind::Calendar, route: format!("#/calendar?event={id}"),
                id, title: row.get(1)?, status: None, archived: false })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?);
    let mut unavailable_spaces = Vec::new();
    for space in spaces {
        let result = inner.with_space(&space.id, |conn| {
            let mut statement = conn.prepare(
                "SELECT p.id, p.title, s.id, s.is_encrypted FROM pages p
                 JOIN sections s ON s.id = p.section_id WHERE p.is_deleted = 0 AND EXISTS
                 (SELECT 1 FROM taggings g WHERE g.entity_type = 'page' AND g.entity_id = p.id AND g.tag_id = ?1)
                 ORDER BY p.id")?;
            let rows = statement.query_map([tag_id], |row| Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, bool>(3)?,
            )))?;
            let mut accessible = Vec::new();
            for row in rows {
                let (id, title, section_id, encrypted) = row?;
                if encrypted && !inner.session.is_unlocked(&section_id) { continue; }
                accessible.push(TaggedItem { kind: TaggedKind::Note,
                    route: format!("#/notes/s/{}/page/{id}", space.id), id, title,
                    status: None, archived: false });
            }
            Ok(accessible)
        });
        match result {
            Ok(notes) => items.extend(notes),
            Err(_) => unavailable_spaces.push(space.id),
        }
    }
    let mut counts = TagCounts::default();
    for item in &items {
        match item.kind {
            TaggedKind::Note => counts.note += 1,
            TaggedKind::Task => counts.task += 1,
            TaggedKind::Calendar => counts.calendar += 1,
        }
    }
    items.retain(|item| kind.is_none_or(|kind| kind == item.kind));
    Ok(TagResults {
        tag,
        items,
        counts,
        unavailable_spaces,
    })
}

#[tauri::command]
pub fn query_tag(
    state: State<'_, AppState>,
    tag_id: String,
    kind: Option<TaggedKind>,
    include_archived: Option<bool>,
) -> Result<TagResults, VaultError> {
    aggregate_tag(
        &state.inner,
        &tag_id,
        kind,
        include_archived.unwrap_or(false),
    )
}

#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Result<Vec<Tag>, VaultError> {
    state.inner.with_meta(tags::list)
}

#[tauri::command]
pub fn create_tag(
    state: State<'_, AppState>,
    name: String,
    color: Option<String>,
) -> Result<Tag, VaultError> {
    state
        .inner
        .with_meta(|meta| tags::create(meta, &name, color.as_deref()))
}

#[tauri::command]
pub fn update_tag(
    state: State<'_, AppState>,
    id: String,
    name: String,
    color: Option<String>,
) -> Result<Tag, VaultError> {
    state
        .inner
        .with_meta(|meta| tags::update(meta, &id, &name, color.as_deref()))
}

/// Local links are removed before the dictionary record. This is intentionally
/// retry-safe: a failed middle step leaves the dictionary intact and a retry
/// clears the remaining links without deleting any tagged entity.
#[tauri::command]
pub fn delete_tag(state: State<'_, AppState>, id: String) -> Result<(), VaultError> {
    let spaces = state.inner.with_meta(|meta| {
        if !tags::exists(meta, &id)? {
            return Err(VaultError::NotFound(format!("标签 {id}")));
        }
        registry::list_spaces(meta)
    })?;
    state.inner.with_tasks(|conn| tags::clear_tag(conn, &id))?;
    state
        .inner
        .with_calendar(|conn| tags::clear_tag(conn, &id))?;
    for space in spaces {
        state
            .inner
            .with_space(&space.id, |conn| tags::clear_tag(conn, &id))?;
    }
    state.inner.with_meta(|meta| tags::delete(meta, &id))
}

#[tauri::command]
pub fn set_page_tags(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
    tag_ids: Vec<String>,
) -> Result<(), VaultError> {
    state.inner.with_space(&space_id, |conn| {
        ensure_page_access(conn, &state.inner.session, &page_id)?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pages WHERE id = ?1 AND is_deleted = 0)",
            [&page_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(VaultError::NotFound(format!("页面 {page_id}")));
        }
        tags::set_entity_tags(conn, "page", &page_id, &tag_ids)
    })
}

#[tauri::command]
pub fn get_page_tags(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<Vec<String>, VaultError> {
    state.inner.with_space(&space_id, |conn| {
        ensure_page_access(conn, &state.inner.session, &page_id)?;
        tags::entity_tags(conn, "page", &page_id)
    })
}

fn ensure_page_access(
    conn: &rusqlite::Connection,
    session: &crate::notes::session::SessionManager,
    page_id: &str,
) -> Result<(), VaultError> {
    use rusqlite::OptionalExtension;
    let section = conn.query_row(
        "SELECT s.id, s.is_encrypted FROM pages p JOIN sections s ON s.id = p.section_id WHERE p.id = ?1 AND p.is_deleted = 0",
        [page_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
    ).optional()?.ok_or_else(|| VaultError::NotFound(format!("页面 {page_id}")))?;
    if section.1 && !session.is_unlocked(&section.0) {
        return Err(VaultError::SectionLocked(section.0));
    }
    Ok(())
}

#[tauri::command]
pub fn set_event_tags(
    state: State<'_, AppState>,
    event_id: String,
    tag_ids: Vec<String>,
) -> Result<(), VaultError> {
    state.inner.with_calendar(|conn| {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE id = ?1)",
            [&event_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(VaultError::NotFound(format!("日程 {event_id}")));
        }
        tags::set_entity_tags(conn, "event", &event_id, &tag_ids)
    })
}

#[tauri::command]
pub fn get_event_tags(
    state: State<'_, AppState>,
    event_id: String,
) -> Result<Vec<String>, VaultError> {
    state
        .inner
        .with_calendar(|conn| tags::entity_tags(conn, "event", &event_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::{layout, startup},
        notes::hierarchy,
        tasks::{lists, tasks},
    };

    #[test]
    fn aggregate_filters_archives_and_locked_pages_without_count_leaks() {
        let directory = tempfile::tempdir().unwrap();
        let root = layout::data_root(directory.path());
        let app = AppState::init(root.clone(), startup::startup(&root).unwrap()).unwrap();
        let inner = &app.inner;
        let tag = inner
            .with_meta(|conn| tags::create(conn, "Shared", None))
            .unwrap();
        let space = inner.with_meta(registry::list_spaces).unwrap().remove(0);
        let section_id = inner
            .with_space(&space.id, |conn| {
                let notebook = hierarchy::create_notebook(conn, "Notebook", None)?;
                let section = hierarchy::create_section(conn, &notebook.id, None, "Secret", None)?;
                let page = hierarchy::create_page(conn, &section.id, None, "Private title")?;
                tags::set_entity_tags(conn, "page", &page.id, &[tag.id.clone()])?;
                conn.execute(
                    "UPDATE sections SET is_encrypted = 1 WHERE id = ?1",
                    [&section.id],
                )?;
                Ok(section.id)
            })
            .unwrap();
        inner
            .with_tasks(|conn| {
                let list = lists::list_lists(conn)?.remove(0);
                for archived in [false, true] {
                    let task = tasks::create_task(conn, &list.id, None, "Task")?;
                    tags::set_entity_tags(conn, "task", &task.id, &[tag.id.clone()])?;
                    if archived {
                        conn.execute(
                            "UPDATE tasks SET archived_at = datetime('now') WHERE id = ?1",
                            [&task.id],
                        )?;
                    }
                }
                Ok(())
            })
            .unwrap();
        inner.with_calendar(|conn| {
            conn.execute("INSERT INTO events(id,title,start_at,end_at) VALUES ('event','Event','2026-01-01','2026-01-02')", [])?;
            tags::set_entity_tags(conn, "event", "event", &[tag.id.clone()])
        }).unwrap();
        let locked = aggregate_tag(inner, &tag.id, None, false).unwrap();
        assert_eq!(
            (
                locked.counts.note,
                locked.counts.task,
                locked.counts.calendar
            ),
            (0, 1, 1)
        );
        assert!(!serde_json::to_string(&locked)
            .unwrap()
            .contains("Private title"));
        let (wrapped, _) = crate::crypto::keys::wrap_dsk_with_params(
            "password",
            &crate::crypto::kdf::KdfParams {
                m_cost: 8192,
                t_cost: 1,
                p_cost: 1,
            },
        )
        .unwrap();
        inner
            .session
            .unlock_with_password(&section_id, &wrapped, "password")
            .unwrap();
        let unlocked = aggregate_tag(inner, &tag.id, None, true).unwrap();
        assert_eq!(
            (
                unlocked.counts.note,
                unlocked.counts.task,
                unlocked.counts.calendar
            ),
            (1, 2, 1)
        );
        assert!(unlocked
            .items
            .iter()
            .all(|item| item.route.starts_with("#/")));
        assert_eq!(
            unlocked.items.iter().filter(|item| item.archived).count(),
            1
        );
        let filtered = aggregate_tag(inner, &tag.id, Some(TaggedKind::Note), false).unwrap();
        assert_eq!(filtered.items.len(), 1);
        assert_eq!(filtered.counts.task, 1);
        inner.session.lock(&section_id);
        let relocked = aggregate_tag(inner, &tag.id, Some(TaggedKind::Note), false).unwrap();
        assert!(relocked.items.is_empty());
        assert_eq!(relocked.counts.note, 0);
        inner.with_meta(|conn| tags::delete(conn, &tag.id)).unwrap();
        let orphan = aggregate_tag(inner, &tag.id, None, false).unwrap();
        assert!(orphan.tag.is_none());
        assert_eq!(orphan.items.len(), 2);
        drop(app);
        let app = AppState::init(root.clone(), startup::startup(&root).unwrap()).unwrap();
        let reopened = aggregate_tag(&app.inner, &tag.id, None, false).unwrap();
        assert_eq!(reopened.items.len(), 2);
        assert_eq!(reopened.counts.note, 0);
        for item in reopened.items {
            match item.kind {
                TaggedKind::Task => app
                    .inner
                    .with_tasks(|conn| tags::set_entity_tags(conn, "task", &item.id, &[]))
                    .unwrap(),
                TaggedKind::Calendar => app
                    .inner
                    .with_calendar(|conn| tags::set_entity_tags(conn, "event", &item.id, &[]))
                    .unwrap(),
                TaggedKind::Note => unreachable!(),
            }
        }
        assert!(aggregate_tag(&app.inner, &tag.id, None, false)
            .unwrap()
            .items
            .is_empty());
    }
}
