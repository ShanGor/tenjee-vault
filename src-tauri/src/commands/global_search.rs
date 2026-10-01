//! Cross-domain search bridge.  It fans out to the three isolated data domains and never
//! materializes an encrypted page outside that space connection's temporary FTS index.

use std::thread;

use serde::Serialize;
use tauri::State;

use super::AppState;
use crate::db::registry;
use crate::error::{VaultError, VaultResult};
use crate::search::{self, EventSearchHit, SearchHit, TaskSearchHit};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GlobalHitKind {
    Note,
    Task,
    Calendar,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GlobalHit {
    pub kind: GlobalHitKind,
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub context: String,
    pub score: f32,
    pub route: String,
}

fn score(index: usize, count: usize) -> f32 {
    1.0 - index as f32 / count.max(1) as f32
}

fn notes_for_space(
    inner: &super::AppStateInner,
    space: &registry::SpaceInfo,
    query: &str,
    limit: usize,
) -> VaultResult<Vec<GlobalHit>> {
    inner.with_space(&space.id, |conn| {
        let unlocked = inner.session.unlocked_section_ids();
        let hits: Vec<SearchHit> = search::search(conn, query, &unlocked, limit)?;
        let count = hits.len();
        Ok(hits
            .into_iter()
            .enumerate()
            .map(|(index, hit)| GlobalHit {
                kind: GlobalHitKind::Note,
                id: hit.page_id.clone(),
                title: hit.title,
                snippet: hit.snippet,
                context: format!("{} / {}",space.name,crate::notes::page_tree::path(conn,&hit.page_id).unwrap_or_default()),
                score: score(index, count),
                route: format!("#/notes/s/{}/page/{}", space.id, hit.page_id),
            })
            .collect())
    })
}

fn tasks_to_global(hits: Vec<TaskSearchHit>) -> Vec<GlobalHit> {
    let count = hits.len();
    hits.into_iter()
        .enumerate()
        .map(|(index, hit)| GlobalHit {
            kind: GlobalHitKind::Task,
            id: hit.task_id.clone(),
            title: hit.title,
            snippet: hit.snippet,
            context: if hit.archived {
                "归档任务".into()
            } else {
                "任务".into()
            },
            score: score(index, count),
            route: format!("#/tasks?task={}", hit.task_id),
        })
        .collect()
}

fn events_to_global(hits: Vec<EventSearchHit>) -> Vec<GlobalHit> {
    let count = hits.len();
    hits.into_iter()
        .enumerate()
        .map(|(index, hit)| GlobalHit {
            kind: GlobalHitKind::Calendar,
            id: hit.event_id.clone(),
            title: hit.title,
            snippet: hit.snippet,
            context: "日程".into(),
            score: score(index, count),
            route: format!("#/calendar?event={}", hit.event_id),
        })
        .collect()
}

/// Preserve a small type quota before filling the remaining slots by normalized domain rank.
fn merge_with_quotas(mut groups: Vec<Vec<GlobalHit>>, limit: usize) -> Vec<GlobalHit> {
    let quota = (limit / 3).max(1);
    let mut merged = Vec::with_capacity(limit);
    for group in &mut groups {
        let take = quota.min(group.len());
        merged.extend(group.drain(..take));
    }
    let mut remaining: Vec<_> = groups.into_iter().flatten().collect();
    remaining.sort_by(|left, right| right.score.total_cmp(&left.score));
    merged.extend(
        remaining
            .into_iter()
            .take(limit.saturating_sub(merged.len())),
    );
    merged.truncate(limit);
    merged
}

#[tauri::command]
pub fn global_search_cmd(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<GlobalHit>, VaultError> {
    global_search(&state.inner, &query, limit.unwrap_or(30))
}

pub fn global_search(
    inner: &super::AppStateInner,
    query: &str,
    limit: usize,
) -> VaultResult<Vec<GlobalHit>> {
    let limit = limit.clamp(1, 100);
    let per_domain = (limit / 3).max(3);
    let spaces = inner.with_meta(registry::list_spaces)?;
    // Database mutexes keep each domain internally safe; the independent domain queries still
    // overlap so a slow/unavailable space does not serialize tasks and calendar results.
    let (tasks, events, notes) = thread::scope(|scope| {
        let tasks = scope.spawn(|| {
            inner.with_tasks(|conn| search::search_tasks(conn, &query, false, per_domain))
        });
        let events = scope
            .spawn(|| inner.with_calendar(|conn| search::search_events(conn, &query, per_domain)));
        let notes = scope.spawn(|| {
            // Spaces are independent databases. Fan each one out as well so a slow space does
            // not serialize every other note result behind it. A missing/isolated space remains
            // a partial-result condition, rather than making the command fail.
            thread::scope(|space_scope| {
                let workers: Vec<_> = spaces
                    .iter()
                    .map(|space| {
                        space_scope.spawn(|| notes_for_space(inner, space, &query, per_domain))
                    })
                    .collect();
                let mut out = Vec::new();
                for worker in workers {
                    if let Ok(Ok(mut hits)) = worker.join() {
                        out.append(&mut hits);
                    }
                }
                Ok::<_, VaultError>(out)
            })
        });
        (tasks.join(), events.join(), notes.join())
    });
    let tasks = tasks.map_err(|_| VaultError::Validation("任务搜索线程异常终止".into()))??;
    let events = events.map_err(|_| VaultError::Validation("日程搜索线程异常终止".into()))??;
    let notes = notes.map_err(|_| VaultError::Validation("笔记搜索线程异常终止".into()))??;
    Ok(merge_with_quotas(
        vec![notes, tasks_to_global(tasks), events_to_global(events)],
        limit,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(kind: GlobalHitKind, id: &str, score: f32) -> GlobalHit {
        GlobalHit {
            kind,
            id: id.into(),
            title: id.into(),
            snippet: String::new(),
            context: String::new(),
            score,
            route: String::new(),
        }
    }

    #[test]
    fn merge_reserves_each_available_domain_before_truncation() {
        let merged = merge_with_quotas(
            vec![
                vec![
                    hit(GlobalHitKind::Note, "n1", 1.0),
                    hit(GlobalHitKind::Note, "n2", 0.9),
                ],
                vec![hit(GlobalHitKind::Task, "t1", 1.0)],
                vec![hit(GlobalHitKind::Calendar, "c1", 1.0)],
            ],
            3,
        );
        assert_eq!(merged.len(), 3);
        assert!(merged
            .iter()
            .any(|item| matches!(item.kind, GlobalHitKind::Note)));
        assert!(merged
            .iter()
            .any(|item| matches!(item.kind, GlobalHitKind::Task)));
        assert!(merged
            .iter()
            .any(|item| matches!(item.kind, GlobalHitKind::Calendar)));
    }

    #[test]
    fn searches_all_domains_and_skips_an_unavailable_space() {
        use crate::{
            calendar::events,
            commands::AppState,
            db::{layout, startup},
            notes::{hierarchy, pages},
            tasks::{lists, tasks},
        };
        let directory = tempfile::tempdir().unwrap();
        let root = layout::data_root(directory.path());
        let app = AppState::init(root.clone(), startup::startup(&root).unwrap()).unwrap();
        let space = app
            .inner
            .with_meta(registry::list_spaces)
            .unwrap()
            .remove(0);
        let (notebook, section) = app
            .inner
            .with_space(&space.id, |conn| {
                let notebook = hierarchy::create_notebook(conn, "测试", None)?;
                let section = hierarchy::create_section(conn, &notebook.id, None, "收集", None)?;
                Ok((notebook, section))
            })
            .unwrap();
        let _ = notebook;
        let page = app
            .inner
            .with_space(&space.id, |conn| {
                hierarchy::create_page(conn, &section.id, None, "共享关键词笔记")
            })
            .unwrap();
        app.inner
            .with_space(&space.id, |conn| {
                pages::save_page(
                    conn,
                    &app.inner.session,
                    &page.id,
                    "共享关键词笔记",
                    "共享关键词正文",
                )
            })
            .unwrap();
        app.inner
            .with_tasks(|conn| {
                let list = lists::list_lists(conn)?.remove(0);
                tasks::create_task(conn, &list.id, None, "共享关键词任务")?;
                Ok(())
            })
            .unwrap();
        app.inner
            .with_calendar(|conn| {
                events::create_event(
                    conn,
                    "共享关键词日程",
                    None,
                    None,
                    "2026-01-01",
                    "2026-01-01",
                    true,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )?;
                Ok(())
            })
            .unwrap();
        let missing = app
            .inner
            .with_meta(|meta| registry::create_space(meta, &app.inner.root, "失效空间"))
            .unwrap();
        std::fs::remove_file(app.inner.root.join(missing.db_file)).unwrap();
        let hits = global_search(&app.inner, "共享关键词", 30).unwrap();
        assert!(hits
            .iter()
            .any(|hit| matches!(hit.kind, GlobalHitKind::Note)));
        assert!(hits
            .iter()
            .any(|hit| matches!(hit.kind, GlobalHitKind::Task)));
        assert!(hits
            .iter()
            .any(|hit| matches!(hit.kind, GlobalHitKind::Calendar)));
    }
}
