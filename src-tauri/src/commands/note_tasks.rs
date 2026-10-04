//! Recoverable cross-database note/task links. The tasks queue never stores note content.
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use tauri::State;
use super::{AppState, AppStateInner};
use crate::{error::{VaultError, VaultResult}, notes::pages, tasks::tasks};

fn invalid() -> VaultError { VaultError::Validation("Invalid or missing linked todo item".into()) }
fn accessible_page(inner: &AppStateInner, conn: &rusqlite::Connection, page_id: &str) -> VaultResult<pages::Page> {
    let (section,encrypted): (String,bool) = conn.query_row("SELECT s.id,s.is_encrypted FROM pages p JOIN sections s ON s.id=p.section_id WHERE p.id=?1 AND p.is_deleted=0",[page_id],|row|Ok((row.get(0)?,row.get(1)?))).optional()?.ok_or_else(||VaultError::NotFound("Source page".into()))?;
    if encrypted && !inner.session.is_unlocked(&section) { return Err(VaultError::SectionLocked(section)); }
    pages::get_page(conn,&inner.session,page_id)
}
fn find<'a>(node: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    if node.get("type").and_then(Value::as_str) == Some("taskItem") && node.pointer("/attrs/nodeId").and_then(Value::as_str) == Some(id) { return Some(node); }
    for child in node.get_mut("content").and_then(Value::as_array_mut).into_iter().flatten() {
        if let Some(found) = find(child,id) { return Some(found); }
    }
    None
}
fn text(node: &Value) -> String {
    if let Some(value) = node.get("text").and_then(Value::as_str) { return value.into(); }
    node.get("content").and_then(Value::as_array).map(|children| children.iter().map(text).collect::<Vec<_>>().join(" ")).unwrap_or_default()
}

pub fn link(inner: &AppStateInner, space: &str, page_id: &str, node_id: &str, list_id: &str) -> VaultResult<String> {
    let reference = format!("{space}:{page_id}");
    let (title, checked, linked_id) = inner.with_space(space, |conn| {
        let page = accessible_page(inner,conn,page_id)?;
        let mut doc: Value = serde_json::from_str(&page.content).map_err(|_| invalid())?;
        let node = find(&mut doc,node_id).ok_or_else(invalid)?;
        Ok((text(node),node.pointer("/attrs/checked").and_then(Value::as_bool).unwrap_or(false),node.pointer("/attrs/taskId").and_then(Value::as_str).map(str::to_owned)))
    })?;
    let task_id = inner.with_tasks(|conn| {
        if let Some(id) = conn.query_row("SELECT id FROM tasks WHERE source_page_ref=?1 AND source_node_id=?2",params![reference,node_id],|row|row.get::<_,String>(0)).optional()? {
            if linked_id.as_deref().is_some_and(|linked| linked != id) { return Err(invalid()); }
            conn.execute("INSERT INTO note_sync_queue(id,task_id,source_page_ref,source_node_id,checked) SELECT ?1,id,source_page_ref,source_node_id,status='done' FROM tasks WHERE id=?2 ON CONFLICT(task_id,source_page_ref,source_node_id) DO UPDATE SET checked=excluded.checked,status='pending',last_error=NULL",params![crate::notes::new_id(),id])?;
            return Ok(id);
        }
        if linked_id.is_some() { return Err(invalid()); }
        let transaction = conn.unchecked_transaction()?;
        let task = tasks::create_task(&transaction,list_id,None,&title)?;
        transaction.execute("UPDATE tasks SET source_page_ref=?1,source_node_id=?2,status=?3,completed_at=CASE WHEN ?3='done' THEN datetime('now') ELSE NULL END WHERE id=?4",params![reference,node_id,if checked {"done"} else {"todo"},task.id])?;
        transaction.execute("INSERT OR IGNORE INTO note_sync_queue(id,task_id,source_page_ref,source_node_id,checked) VALUES (?1,?2,?3,?4,?5)",params![crate::notes::new_id(),task.id,reference,node_id,checked])?;
        transaction.commit()?;
        Ok(task.id)
    })?;
    sync(inner)?;
    if inspect(inner,&task_id)?.state != "linked" { return Err(invalid()); }
    Ok(task_id)
}

/// Queue reads and writes use short domain locks. Conditional deletion leaves a newer
/// queued state intact if another status change arrives while the page is being updated.
pub fn sync(inner: &AppStateInner) -> VaultResult<()> {
    let queued = inner.with_tasks(|conn| {
        let mut statement = conn.prepare("SELECT q.id,q.task_id,q.source_page_ref,q.source_node_id,q.checked,q.source_context FROM note_sync_queue q JOIN tasks t ON t.id=q.task_id WHERE q.status='pending' AND t.source_page_ref=q.source_page_ref AND t.source_node_id=q.source_node_id ORDER BY q.created_at,q.id")?;
        let rows = statement.query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,bool>(4)?,row.get::<_,Option<String>>(5)?)))?.collect::<Result<Vec<_>,_>>()?;
        Ok(rows)
    })?;
    for (queue_id,task_id,reference,node_id,checked,source_context) in queued {
        let result = (|| {
            let (space,page_id) = reference.split_once(':').ok_or_else(invalid)?;
            inner.with_space(space, |conn| {
                let page = accessible_page(inner,conn,page_id)?;
                let mut document: Value = serde_json::from_str(&page.content).map_err(|_| invalid())?;
                let node = find(&mut document,&node_id).ok_or_else(invalid)?;
                if let Some(existing) = node.pointer("/attrs/taskId").and_then(Value::as_str) {
                    if existing != task_id { return Err(invalid()); }
                }
                node["attrs"]["taskId"] = Value::String(task_id.clone());
                node["attrs"]["checked"] = Value::Bool(checked);
                conn.execute("INSERT INTO note_task_projections(task_id,page_id,node_id,checked,source_context) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(task_id,page_id,node_id) DO UPDATE SET checked=excluded.checked,source_context=excluded.source_context",params![task_id,page_id,node_id,checked,source_context.as_deref().unwrap_or("{}")])?;
                Ok(())
            })
        })();
        match result {
            Ok(()) => inner.with_tasks(|conn| { conn.execute("DELETE FROM note_sync_queue WHERE id=?1 AND checked=?2 AND source_context IS ?3",params![queue_id,checked,source_context])?; Ok(()) })?,
            Err(VaultError::SectionLocked(_) | VaultError::NotFound(_)) => {},
            Err(_) => inner.with_tasks(|conn| { conn.execute("UPDATE note_sync_queue SET status='failed',attempts=attempts+1,last_error='source_unavailable' WHERE id=?1 AND checked=?2 AND source_context IS ?3",params![queue_id,checked,source_context])?; Ok(()) })?,
        }
    }
    Ok(())
}

pub fn from_note(inner: &AppStateInner, space: &str, page_id: &str, content: &str) -> VaultResult<()> {
    let Ok(mut document) = serde_json::from_str::<Value>(content) else { return Ok(()); };
    let reference = format!("{space}:{page_id}");
    inner.with_tasks(|conn| {
        let links = {
            let mut statement = conn.prepare("SELECT id,source_node_id,status FROM tasks WHERE source_page_ref=?1")?;
            let rows = statement.query_map([&reference],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?)))?.collect::<Result<Vec<_>,_>>()?;
            rows
        };
        for (task_id,node_id,status) in links {
            if let Some(node) = find(&mut document,&node_id) {
                if node.pointer("/attrs/taskId").and_then(Value::as_str) != Some(&task_id) { continue; }
                let checked = node.pointer("/attrs/checked").and_then(Value::as_bool).unwrap_or(false);
                if checked != (status == "done") { tasks::set_task_status(conn,&task_id,if checked {"done"} else {"todo"})?; }
            }
        }
        Ok(())
    })
}

#[derive(Serialize)]
pub struct Link { pub task_id: String, pub node_id: String, pub state: String, pub route: Option<String> }

pub fn inspect(inner: &AppStateInner, task_id: &str) -> VaultResult<Link> {
    let source = inner.with_tasks(|conn| Ok(conn.query_row("SELECT source_page_ref,source_node_id FROM tasks WHERE id=?1",[task_id],|row|Ok((row.get::<_,Option<String>>(0)?,row.get::<_,Option<String>>(1)?))).optional()?))?;
    if source.as_ref().is_some_and(|(page,node)| page.is_none() || node.is_none()) { return Ok(Link { task_id:task_id.into(),node_id:String::new(),state:"none".into(),route:None }); }
    let Some((Some(reference),Some(node_id))) = source else { return Ok(Link { task_id:task_id.into(),node_id:String::new(),state:"missing".into(),route:None }); };
    let Some((space,page_id)) = reference.split_once(':') else { return Err(invalid()); };
    let found = inner.with_space(space, |conn| {
        let page = accessible_page(inner,conn,page_id)?;
        let mut doc: Value = serde_json::from_str(&page.content).map_err(|_| invalid())?;
        Ok(find(&mut doc,&node_id).and_then(|node|node.pointer("/attrs/taskId").and_then(Value::as_str)) == Some(task_id))
    });
    let (state,route) = match found {
        Ok(true) => ("linked",Some(format!("#/notes/s/{space}/page/{page_id}"))),
        Err(VaultError::SectionLocked(_)) => ("locked",None),
        _ => ("missing",None),
    };
    Ok(Link { task_id:task_id.into(),node_id,state:state.into(),route })
}

pub fn unlink(inner: &AppStateInner, task_id: &str) -> VaultResult<()> {
    let source = inner.with_tasks(|conn| Ok(conn.query_row("SELECT source_page_ref,source_node_id FROM tasks WHERE id=?1",[task_id],|row|Ok((row.get::<_,Option<String>>(0)?,row.get::<_,Option<String>>(1)?))).optional()?))?;
    if let Some((Some(reference),Some(node_id))) = source {
        if let Some((space,page_id)) = reference.split_once(':') {
            let result = inner.with_space(space, |conn| {
                let page = accessible_page(inner,conn,page_id)?;
                let mut doc: Value = serde_json::from_str(&page.content).map_err(|_| invalid())?;
                if let Some(node) = find(&mut doc,&node_id) {
                    if node.pointer("/attrs/taskId").and_then(Value::as_str) == Some(task_id) { node["attrs"]["taskId"] = Value::Null; }
                }
                pages::save_page(conn,&inner.session,page_id,&page.title,&doc.to_string())
            });
            if matches!(result,Err(VaultError::SectionLocked(_))) { return Err(VaultError::SectionLocked("source".into())); }
            if let Err(error) = result { if !matches!(error,VaultError::NotFound(_) | VaultError::Validation(_)) { return Err(error); } }
        }
    }
    inner.with_tasks(|conn| {
        let transaction = conn.unchecked_transaction()?;
        transaction.execute("UPDATE tasks SET source_page_ref=NULL,source_node_id=NULL WHERE id=?1",[task_id])?;
        transaction.execute("DELETE FROM note_sync_queue WHERE task_id=?1",[task_id])?;
        transaction.commit()?; Ok(())
    })
}

#[tauri::command]
pub fn link_note_task(state: State<'_,AppState>, space_id:String,page_id:String,node_id:String,list_id:String) -> VaultResult<String> { link(&state.inner,&space_id,&page_id,&node_id,&list_id) }
#[tauri::command]
pub fn inspect_note_task(state: State<'_,AppState>, task_id:String) -> VaultResult<Link> { inspect(&state.inner,&task_id) }
#[tauri::command]
pub fn unlink_note_task(state: State<'_,AppState>, task_id:String) -> VaultResult<()> { unlink(&state.inner,&task_id) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::{layout,startup,registry},notes::{hierarchy,sections_crypto},tasks::lists};
    use serde_json::json;
    #[test]
    fn links_retry_sync_in_both_directions_and_defer_locked_sources_without_content() {
        let directory = tempfile::tempdir().unwrap();
        let root = layout::data_root(directory.path());
        let app = AppState::init(root.clone(),startup::startup(&root).unwrap()).unwrap();
        let inner = &app.inner;
        let space = inner.with_meta(registry::list_spaces).unwrap().remove(0);
        let list = inner.with_tasks(|conn| Ok(lists::list_lists(conn)?.remove(0).id)).unwrap();
        for encrypted in [false,true] {
            let (section,page) = inner.with_space(&space.id,|conn| {
                let notebook = hierarchy::create_notebook(conn,"N",None)?;
                let section = hierarchy::create_section(conn,&notebook.id,None,"S",None)?;
                let page = hierarchy::create_page(conn,&section.id,None,"Private page")?;
                let content = json!({"type":"doc","content":[{"type":"taskList","content":[{"type":"taskItem","attrs":{"nodeId":"node","checked":false},"content":[{"type":"paragraph","content":[{"type":"text","text":"private todo"}]}]}]}]}).to_string();
                pages::save_page(conn,&inner.session,&page.id,"Private page",&content)?;
                if encrypted { let keys = sections_crypto::set_password(conn,&section.id,"password",true)?; inner.session.insert(&section.id,keys); }
                Ok((section.id,page.id))
            }).unwrap();
            let task = link(inner,&space.id,&page,"node",&list).unwrap();
            assert_eq!(link(inner,&space.id,&page,"node",&list).unwrap(),task);
            assert_eq!(inspect(inner,&task).unwrap().state,"linked");
            // Simulate an interrupted first write-back. Retry uses the existing task.
            inner.with_space(&space.id,|conn| {
                let source = pages::get_page(conn,&inner.session,&page)?;
                let mut doc: Value = serde_json::from_str(&source.content).unwrap();
                find(&mut doc,"node").unwrap()["attrs"]["taskId"] = Value::Null;
                pages::save_page(conn,&inner.session,&page,&source.title,&doc.to_string())
            }).unwrap();
            assert_eq!(link(inner,&space.id,&page,"node",&list).unwrap(),task);
            if encrypted { inner.session.lock(&section); }
            inner.with_tasks(|conn|tasks::set_task_status(conn,&task,"done")).unwrap();
            sync(inner).unwrap();
            if encrypted {
                assert_eq!(inspect(inner,&task).unwrap().state,"locked");
                assert!(inspect(inner,&task).unwrap().route.is_none());
                inner.with_tasks(|conn| {
                    let count: i64 = conn.query_row("SELECT count(*) FROM note_sync_queue WHERE task_id=?1 AND status='pending'",[&task],|row|row.get(0))?;
                    assert_eq!(count,1);
                    let queued: String = conn.query_row("SELECT source_page_ref || source_node_id || coalesce(last_error,'') FROM note_sync_queue WHERE task_id=?1",[&task],|row|row.get(0))?;
                    assert!(!queued.contains("private"));
                    Ok(())
                }).unwrap();
                inner.with_space(&space.id,|conn| { inner.session.insert(&section,sections_crypto::unlock_keys(conn,&section,"password")?); Ok(()) }).unwrap();
            }
            sync(inner).unwrap(); sync(inner).unwrap();
            let content = inner.with_space(&space.id,|conn| {
                let source = pages::get_page(conn,&inner.session,&page)?;
                let mut doc: Value = serde_json::from_str(&source.content).unwrap();
                assert_eq!(find(&mut doc,"node").unwrap()["attrs"]["checked"],true);
                find(&mut doc,"node").unwrap()["attrs"]["checked"] = Value::Bool(false);
                pages::save_page(conn,&inner.session,&page,&source.title,&doc.to_string())?;
                Ok(doc.to_string())
            }).unwrap();
            from_note(inner,&space.id,&page,&content).unwrap();
            assert_eq!(inner.with_tasks(|conn| Ok(conn.query_row("SELECT status FROM tasks WHERE id=?1",[&task],|row|row.get::<_,String>(0))?)).unwrap(),"todo");
            unlink(inner,&task).unwrap();
            assert_eq!(inspect(inner,&task).unwrap().state,"none");
            let second = link(inner,&space.id,&page,"node",&list).unwrap();
            inner.with_space(&space.id,|conn| { conn.execute("UPDATE pages SET is_deleted=1 WHERE id=?1",[&page])?; Ok(()) }).unwrap();
            assert_eq!(inspect(inner,&second).unwrap().state,"missing");
            inner.with_tasks(|conn|tasks::set_task_status(conn,&second,"done")).unwrap();
            sync(inner).unwrap();
            assert_eq!(inner.with_tasks(|conn| Ok(conn.query_row("SELECT status FROM note_sync_queue WHERE task_id=?1",[&second],|row|row.get::<_,String>(0))?)).unwrap(),"failed");
            unlink(inner,&second).unwrap();
        }
    }
}
