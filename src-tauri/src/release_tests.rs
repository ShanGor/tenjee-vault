//! Upgrade and restore fixture built with the last M3 migration of each database.
use crate::{backup::writer, blob_store, commands::AppState, db::{connection,layout,migrate::{run_migrations,DbKind},restore,startup},notes::{attachments,hierarchy,pages,sections_crypto,session::SessionManager},tasks::{lists,tasks}};
use rusqlite::params;
use serde_json::{json,Value};
use std::path::Path;

const SPACE: &str = "11111111-1111-4111-8111-111111111111";

fn fixture(root: &Path) {
    layout::ensure_layout(root).unwrap();
    for (file,kind,count) in [("meta.db",DbKind::Meta,3),("tasks.db",DbKind::Tasks,2),("calendar.db",DbKind::Calendar,2)] {
        let mut conn=connection::open_db(&root.join(file)).unwrap();
        run_migrations(&mut conn,&kind.migrations()[..count]).unwrap();
    }
    let meta=connection::open_db(&root.join("meta.db")).unwrap();
    meta.execute("INSERT INTO spaces(id,name,db_file) VALUES (?1,'M3 space',?2)",params![SPACE,format!("spaces/{SPACE}.db")]).unwrap();
    meta.execute("INSERT INTO tags(id,name) VALUES ('tag','Preserved tag')",[]).unwrap();
    meta.execute("INSERT INTO app_config(key,value) VALUES ('section_auto_lock_minutes','17')",[]).unwrap();
    let mut space=connection::open_db(&root.join(format!("spaces/{SPACE}.db"))).unwrap();
    run_migrations(&mut space,&DbKind::Space.migrations()[..2]).unwrap();
    let notebook=hierarchy::create_notebook(&space,"M3 notebook",None).unwrap();
    let session=SessionManager::new();
    let files=root.join(format!("spaces/{SPACE}.files"));
    for encrypted in [false,true] {
        let section=hierarchy::create_section(&space,&notebook.id,None,if encrypted {"Encrypted"} else {"Plain"},None).unwrap();
        let page=hierarchy::create_page(&space,&section.id,None,"M3 page").unwrap();
        pages::save_page(&mut space,&session,&page.id,"M3 page",r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"M3 preserved content"}]}]}"#).unwrap();
        if encrypted {
            let keys=sections_crypto::set_password_with_params(&mut space,&section.id,"password",true,&crate::crypto::kdf::KdfParams {m_cost:8192,t_cost:1,p_cost:1}).unwrap();
            session.insert(&section.id,keys);
        }
        attachments::save_attachment(&files,&space,&session,&section.id,"page",&page.id,"fixture.txt",Some("text/plain"),b"preserved attachment").unwrap();
        crate::tags::set_entity_tags(&space,"page",&page.id,&["tag".into()]).unwrap();
    }
    let tasks_conn=connection::open_db(&root.join("tasks.db")).unwrap();
    let list=lists::create_list(&tasks_conn,"M3 tasks",None).unwrap();
    let task=tasks::create_task(&tasks_conn,&list.id,None,"M3 task").unwrap();
    crate::tasks::attachments::save_attachment(&root.join("tasks.files"),&tasks_conn,&task.id,"task.txt",Some("text/plain"),b"task attachment").unwrap();
    crate::tags::set_entity_tags(&tasks_conn,"task",&task.id,&["tag".into()]).unwrap();
    let calendar=connection::open_db(&root.join("calendar.db")).unwrap();
    calendar.execute("INSERT INTO events(id,title,start_at,end_at,all_day) VALUES ('event','M3 event','2026-09-22','2026-09-23',1)",[]).unwrap();
}

fn evidence(root: &Path) -> Value {
    let projection = |file: &str, query: &str| -> Vec<String> {
        let conn=connection::open_db(&root.join(file)).unwrap();
        let mut statement=conn.prepare(query).unwrap();
        statement.query_map([],|row|row.get(0)).unwrap().collect::<Result<Vec<String>,_>>().unwrap()
    };
    let hashes = |directory: &Path| -> Vec<String> {
        let mut hashes:Vec<_>=std::fs::read_dir(directory).unwrap().map(|entry| { let entry=entry.unwrap(); format!("{}:{}",entry.file_name().to_string_lossy(),blob_store::hash_hex(&std::fs::read(entry.path()).unwrap())) }).collect();
        hashes.sort();hashes
    };
    json!({
        "pages":projection(&format!("spaces/{SPACE}.db"),"SELECT json_array(id,section_id,title,content) FROM pages WHERE id NOT LIKE 'notebook:%' AND id NOT LIKE 'group:%' AND id NOT LIKE 'section:%' ORDER BY id"),
        "keys":projection(&format!("spaces/{SPACE}.db"),"SELECT json_array(id,hex(kdf_salt),kdf_params,hex(verifier),hex(wrapped_dsk)) FROM sections WHERE id != '__plain_pages__' ORDER BY id"),
        "note_tags":projection(&format!("spaces/{SPACE}.db"),"SELECT json_array(tag_id,entity_id) FROM taggings ORDER BY entity_id"),
        "tasks":projection("tasks.db","SELECT json_array(id,list_id,title,status) FROM tasks ORDER BY id"),
        "task_tags":projection("tasks.db","SELECT json_array(tag_id,entity_id) FROM taggings ORDER BY entity_id"),
        "events":projection("calendar.db","SELECT json_array(id,title,start_at,end_at,all_day) FROM events ORDER BY id"),
        "tags":projection("meta.db","SELECT json_array(id,name) FROM tags ORDER BY id"),
        "settings":projection("meta.db","SELECT value FROM app_config WHERE key='section_auto_lock_minutes'"),
        "note_blobs":hashes(&root.join(format!("spaces/{SPACE}.files"))),
        "task_blobs":hashes(&root.join("tasks.files")),
    })
}

#[test]
fn m3_upgrade_and_backup_restore_preserve_all_domains_and_ciphertext() {
    let directory=tempfile::tempdir().unwrap();
    let root=layout::data_root(directory.path());
    fixture(&root);
    let before=evidence(&root);
    let report=startup::startup(&root).unwrap();
    assert!(report.alerts.is_empty(),"{report:?}");
    let state=AppState::init(root.clone(),report).unwrap();
    state.inner.with_space(SPACE,|_|Ok(())).unwrap();
    assert_eq!(evidence(&root),before);
    state.inner.with_space(SPACE, |conn| {
        let tree=crate::notes::page_tree::tree(conn)?;
        assert_eq!(tree.len(),1);
        assert_eq!(tree[0].children.len(),2);
        assert!(tree[0].children.iter().all(|node| node.children.len()==1));
        Ok(())
    }).unwrap();
    let archive=directory.path().join("m3-upgraded.tvault");
    writer::create_backup(&state.inner,&archive,false,false).unwrap();
    state.inner.with_tasks(|conn| { conn.execute("UPDATE tasks SET title='Changed after backup'",[])?;Ok(()) }).unwrap();
    restore::prepare_restore(&root,&archive).unwrap();
    drop(state);
    assert!(restore::apply_pending_restore(&root).unwrap());
    let restored=AppState::init(root.clone(),startup::startup(&root).unwrap()).unwrap();
    assert_eq!(evidence(&root),before);
    assert_eq!(crate::commands::settings_of(&restored.inner).unwrap().section_auto_lock_minutes,17);
    restored.inner.with_space(SPACE,|conn| {
        let section:String=conn.query_row("SELECT id FROM sections WHERE is_encrypted=1",[],|row|row.get(0))?;
        let page:String=conn.query_row("SELECT id FROM pages WHERE section_id=?1",[&section],|row|row.get(0))?;
        assert!(pages::get_page(conn,&restored.inner.session,&page).is_err());
        restored.inner.session.insert(&section,sections_crypto::unlock_keys(conn,&section,"password")?);
        assert!(pages::get_page(conn,&restored.inner.session,&page)?.content.contains("M3 preserved content"));
        Ok(())
    }).unwrap();
    println!("release-evidence-sha256={}",blob_store::hash_hex(&serde_json::to_vec(&before).unwrap()));
}
