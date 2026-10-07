//! Template operations own copies of document resources; no source-page lifetime dependency.
use std::collections::{BTreeMap, BTreeSet};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;
use super::{AppState, AppStateInner};
use crate::{crypto::cipher, error::{VaultError, VaultResult}, notes::{self, attachments, hierarchy, pages}};

#[derive(Debug, Serialize)]
pub struct Template { pub id: String, pub name: String, pub scope: String }
#[derive(Serialize, Deserialize)]
struct Resource { name: String, mime: Option<String>, data: String }
#[derive(Serialize, Deserialize)]
struct Payload { document: Value, resources: BTreeMap<String, Resource> }

fn invalid(message: &str) -> VaultError { VaultError::Validation(message.into()) }
fn name(value: &str) -> VaultResult<&str> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 120 { return Err(invalid("Template name must contain 1–120 characters")); }
    Ok(value)
}
fn decode(value: &str) -> VaultResult<Payload> { serde_json::from_str(value).map_err(|_| invalid("Invalid template document")) }
fn encode(value: &Payload) -> VaultResult<String> { serde_json::to_string(value).map_err(|_| invalid("Invalid template document")) }
fn visit(node: &mut Value, f: &mut impl FnMut(&mut Value)) {
    f(node);
    if let Some(children) = node.get_mut("content").and_then(Value::as_array_mut) {
        for child in children { visit(child, f); }
    }
}
fn section_list(inner: &AppStateInner, conn: &Connection, section_id: &str) -> VaultResult<Vec<Template>> {
    if !notes::section_encrypted(conn, section_id)? { return Ok(Vec::new()); }
    if !inner.session.is_unlocked(section_id) { return Ok(Vec::new()); }
    inner.session.with_dsk(section_id, |key| {
        let mut statement = conn.prepare("SELECT id, name_ciphertext FROM section_templates WHERE section_id = ?1 ORDER BY created_at, id")?;
        let mut out = Vec::new();
        for row in statement.query_map([section_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)))? {
            let (id, bytes) = row?;
            let name = String::from_utf8(cipher::open(&bytes, key)?).map_err(|_| invalid("Invalid template name"))?;
            out.push(Template { id, name, scope: "section".into() });
        }
        Ok(out)
    })
}
fn insert_global(conn: &Connection, label: &str, payload: &str) -> VaultResult<String> {
    let label = name(label)?;
    let duplicate: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM page_templates WHERE name = ?1 COLLATE NOCASE)", [label], |row| row.get(0))?;
    if duplicate { return Err(invalid("A template with this name already exists")); }
    let id = notes::new_id();
    conn.execute("INSERT INTO page_templates(id,name,content_json) VALUES (?1,?2,?3)", params![id,label,payload])?;
    Ok(id)
}

pub fn list(inner: &AppStateInner, space_id: &str, section_id: &str) -> VaultResult<Vec<Template>> {
    let mut out = inner.with_meta(|conn| {
        let mut statement = conn.prepare("SELECT id,name FROM page_templates ORDER BY name COLLATE NOCASE")?;
        let rows = statement.query_map([], |row| Ok(Template { id: row.get(0)?, name: row.get(1)?, scope: "global".into() }))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    out.extend(inner.with_space(space_id, |conn| section_list(inner, conn, section_id))?);
    Ok(out)
}

pub fn save(inner: &AppStateInner, space_id: &str, page_id: &str, label: &str, global: bool, confirm_plaintext: bool) -> VaultResult<String> {
    let label = name(label)?;
    let (section_id, encrypted, payload) = inner.with_space(space_id, |conn| {
        let page = pages::get_page(conn, &inner.session, page_id)?;
        let encrypted = notes::section_encrypted(conn, &page.section_id)?;
        if encrypted && global && !confirm_plaintext { return Err(invalid("Confirm plaintext template export first")); }
        let mut document: Value = serde_json::from_str(&page.content).unwrap_or_else(|_| json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":page.content}]}]}));
        if document.get("type").and_then(Value::as_str) != Some("doc") { return Err(invalid("Invalid template document")); }
        let mut ids = BTreeSet::new();
        visit(&mut document, &mut |node| {
            if let Some(id) = node.pointer("/attrs/attachmentId").and_then(Value::as_str) { ids.insert(id.to_owned()); }
        });
        let mut resources = BTreeMap::new();
        for id in ids {
            let resource = attachments::open_attachment(&inner.files_dir(space_id), conn, &inner.session, &id)?;
            if resource.attachment.entity_id != page_id { return Err(invalid("Template attachment belongs to another page")); }
            resources.insert(id, Resource { name: resource.attachment.file_name, mime: resource.attachment.mime, data: resource.data_base64 });
        }
        Ok((page.section_id, encrypted, encode(&Payload { document, resources })?))
    })?;
    if !encrypted || global { return inner.with_meta(|conn| insert_global(conn, label, &payload)); }
    inner.with_space(space_id, |conn| {
        if section_list(inner, conn, &section_id)?.iter().any(|item| item.name.to_lowercase() == label.to_lowercase()) { return Err(invalid("A template with this name already exists")); }
        inner.session.with_dsk(&section_id, |key| {
            let id = notes::new_id();
            conn.execute("INSERT INTO section_templates(id,section_id,name_ciphertext,content_ciphertext) VALUES (?1,?2,?3,?4)", params![id,section_id,cipher::seal(label.as_bytes(),key)?,cipher::seal(payload.as_bytes(),key)?])?;
            Ok(id)
        })
    })
}

fn load(inner: &AppStateInner, space_id: &str, section_id: &str, id: &str, scope: &str) -> VaultResult<Payload> {
    if scope == "global" {
        return inner.with_meta(|conn| {
            let value: Option<String> = conn.query_row("SELECT content_json FROM page_templates WHERE id=?1", [id], |row| row.get(0)).optional()?;
            decode(&value.ok_or_else(|| VaultError::NotFound("Template".into()))?)
        });
    }
    if scope != "section" { return Err(invalid("Invalid template scope")); }
    inner.with_space(space_id, |conn| inner.session.with_dsk(section_id, |key| {
        let bytes: Option<Vec<u8>> = conn.query_row("SELECT content_ciphertext FROM section_templates WHERE id=?1 AND section_id=?2", params![id,section_id], |row| row.get(0)).optional()?;
        let plain = cipher::open(&bytes.ok_or_else(|| VaultError::NotFound("Template".into()))?, key)?;
        decode(&String::from_utf8(plain).map_err(|_| invalid("Invalid template document"))?)
    }))
}

pub fn edit(inner: &AppStateInner, space_id: &str, section_id: &str, id: &str, scope: &str, label: Option<&str>) -> VaultResult<()> {
    if let Some(label) = label { name(label)?; }
    if scope == "global" {
        return inner.with_meta(|conn| {
            let count = if let Some(label) = label {
                let duplicate: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM page_templates WHERE name=?1 COLLATE NOCASE AND id!=?2)", params![label.trim(),id], |row| row.get(0))?;
                if duplicate { return Err(invalid("A template with this name already exists")); }
                conn.execute("UPDATE page_templates SET name=?1, updated_at=datetime('now') WHERE id=?2", params![label.trim(),id])?
            } else { conn.execute("DELETE FROM page_templates WHERE id=?1", [id])? };
            if count == 0 { return Err(VaultError::NotFound("Template".into())); }
            Ok(())
        });
    }
    if scope != "section" { return Err(invalid("Invalid template scope")); }
    inner.with_space(space_id, |conn| {
        if let Some(label) = label {
            if section_list(inner, conn, section_id)?.iter().any(|item| item.id != id && item.name.to_lowercase() == label.trim().to_lowercase()) { return Err(invalid("A template with this name already exists")); }
        }
        inner.session.with_dsk(section_id, |key| {
            let count = if let Some(label) = label { conn.execute("UPDATE section_templates SET name_ciphertext=?1, updated_at=datetime('now') WHERE id=?2 AND section_id=?3", params![cipher::seal(label.trim().as_bytes(),key)?,id,section_id])? }
                else { conn.execute("DELETE FROM section_templates WHERE id=?1 AND section_id=?2", params![id,section_id])? };
            if count == 0 { return Err(VaultError::NotFound("Template".into())); }
            Ok(())
        })
    })
}

pub fn export_global(inner: &AppStateInner, space_id: &str, section_id: &str, id: &str, label: &str, confirmed: bool) -> VaultResult<String> {
    if !confirmed { return Err(invalid("Confirm plaintext template export first")); }
    let payload = load(inner, space_id, section_id, id, "section")?;
    inner.with_meta(|conn| insert_global(conn, label, &encode(&payload)?))
}

pub fn instantiate(inner: &AppStateInner, space_id: &str, section_id: &str, parent: Option<&str>, title: &str, id: &str, scope: &str, builtin: Option<Value>) -> VaultResult<hierarchy::PageSummary> {
    let mut payload = if scope == "builtin" {
        let document = builtin.ok_or_else(|| invalid("Missing built-in template"))?;
        if document.get("type").and_then(Value::as_str) != Some("doc") { return Err(invalid("Invalid template document")); }
        Payload { document, resources: BTreeMap::new() }
    } else { load(inner, space_id, section_id, id, scope)? };
    inner.with_space(space_id, |conn| {
        let encrypted = notes::section_encrypted(conn, section_id)?;
        if encrypted { inner.session.with_dsk(section_id, |_| Ok(()))?; }
        let mut created_hashes = Vec::new();
        let result = (|| {
        let transaction = conn.unchecked_transaction()?;
        let mut page = hierarchy::create_page_prepared(&transaction, section_id, parent, title,false)?;
        page.title=title.into();
        page.sort_order=transaction.query_row("SELECT COALESCE(MAX(sort_order),-1)+1 FROM pages WHERE parent_page_id IS ?1 AND id!=?2 AND is_deleted=0",params![parent,page.id],|r|r.get(0))?;
        transaction.execute("UPDATE pages SET sort_order=?1 WHERE id=?2",params![page.sort_order,page.id])?;
        let mut ids = BTreeMap::new();
        for (old_id, resource) in &payload.resources {
            let attachment = attachments::save_attachment(&inner.files_dir(space_id), &transaction, &inner.session, section_id, "page", &page.id, &resource.name, resource.mime.as_deref(), &notes::base64_decode(&resource.data)?)?;
            created_hashes.push(attachment.hash.clone());
            ids.insert(old_id.clone(), attachment.id);
        }
        let mut missing_resource = false;
        visit(&mut payload.document, &mut |node| {
            let task_item = node.get("type").and_then(Value::as_str) == Some("taskItem");
            if let Some(attrs) = node.get_mut("attrs").and_then(Value::as_object_mut) {
                if let Some(old) = attrs.get("attachmentId").and_then(Value::as_str).map(str::to_owned) {
                    if let Some(id) = ids.get(&old) { attrs.insert("attachmentId".into(), json!(id)); }
                    else { missing_resource = true; }
                }
                if attrs.contains_key("nodeId") || task_item { attrs.insert("nodeId".into(), json!(notes::new_id())); }
                if attrs.contains_key("id") { attrs.insert("id".into(), json!(notes::new_id())); }
                if task_item { attrs.insert("taskId".into(), Value::Null); }
            } else if task_item { node["attrs"] = json!({"nodeId": notes::new_id(), "taskId": null}); }
        });
        if missing_resource { return Err(invalid("Template attachment is missing")); }
        let content = serde_json::to_string(&payload.document).map_err(|_| invalid("Invalid template document"))?;
        let stored = notes::protect_content(&inner.session, section_id, encrypted, &content)?;
        transaction.execute("UPDATE pages SET content=?1 WHERE id=?2", params![stored,page.id])?;
        transaction.commit()?;
        Ok(page)
        })();
        if result.is_err() {
            for hash in created_hashes { crate::blob_store::remove_if_unref(&inner.files_dir(space_id), conn, &hash)?; }
        }
        result
    })
}

#[tauri::command]
pub fn list_templates(state: State<'_,AppState>, space_id: String, section_id: String) -> VaultResult<Vec<Template>> { list(&state.inner,&space_id,&section_id) }
#[tauri::command]
pub fn save_page_template(state: State<'_,AppState>, space_id: String, page_id: String, name: String, global: bool, confirm_plaintext: bool) -> VaultResult<String> { save(&state.inner,&space_id,&page_id,&name,global,confirm_plaintext) }
#[tauri::command]
pub fn edit_template(state: State<'_,AppState>, space_id: String, section_id: String, id: String, scope: String, name: Option<String>) -> VaultResult<()> { edit(&state.inner,&space_id,&section_id,&id,&scope,name.as_deref()) }
#[tauri::command]
pub fn export_template_global(state: State<'_,AppState>, space_id: String, section_id: String, id: String, name: String, confirmed: bool) -> VaultResult<String> { export_global(&state.inner,&space_id,&section_id,&id,&name,confirmed) }
#[tauri::command]
pub fn create_page_from_template(state: State<'_,AppState>, space_id: String, section_id: String, parent_page_id: Option<String>, title: String, id: String, scope: String, builtin: Option<Value>) -> VaultResult<hierarchy::PageSummary> {
    let _ = section_id;
    let domain = state.inner.with_space(&space_id, |conn| notes::page_tree::target_domain(conn,&state.inner.session,parent_page_id.as_deref()))?;
    let page = instantiate(&state.inner,&space_id,&domain,parent_page_id.as_deref(),&title,&id,&scope,builtin)?;
    state.inner.with_space(&space_id, |conn| super::notes::refresh_page_index(&state.inner,conn,&page.id))?;
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::{layout, startup, registry}, notes::sections_crypto};

    #[test]
    fn template_copies_resources_and_nodes_and_survives_source_deletion() {
        let directory = tempfile::tempdir().unwrap();
        let root = layout::data_root(directory.path());
        let app = AppState::init(root.clone(), startup::startup(&root).unwrap()).unwrap();
        let inner = &app.inner;
        let space = inner.with_meta(registry::list_spaces).unwrap().remove(0);
        let (section, source, attachment_id) = inner.with_space(&space.id, |conn| {
            let notebook = hierarchy::create_notebook(conn, "N", None)?;
            let section = hierarchy::create_section(conn, &notebook.id, None, "S", None)?;
            let page = hierarchy::create_page(conn, &section.id, None, "Source")?;
            let attachment = attachments::save_attachment(&inner.files_dir(&space.id),conn,&inner.session,&section.id,"page",&page.id,"file.txt",Some("text/plain"),b"owned resource")?;
            let document = json!({"type":"doc","content":[
                {"type":"taskList","content":[{"type":"taskItem","attrs":{"nodeId":"old-node","taskId":"old-task","checked":true},"content":[{"type":"paragraph","content":[{"type":"text","text":"Checklist"}]}]}]},
                {"type":"attachmentBlock","attrs":{"attachmentId":attachment.id,"fileName":"file.txt"}}
            ]});
            pages::save_page(conn,&inner.session,&page.id,"Source",&document.to_string())?;
            Ok((section.id,page.id,attachment.id))
        }).unwrap();
        let id = save(inner,&space.id,&source,"Checklist",true,false).unwrap();
        assert!(save(inner,&space.id,&source,"checklist",true,false).is_err());
        inner.with_space(&space.id, |conn| {
            attachments::delete_attachment(&inner.files_dir(&space.id),conn,&attachment_id)?;
            conn.execute("DELETE FROM page_versions WHERE page_id=?1",[&source])?;
            conn.execute("DELETE FROM pages WHERE id=?1",[&source])?;
            Ok(())
        }).unwrap();
        let page = instantiate(inner,&space.id,&section,None,"Copy",&id,"global",None).unwrap();
        inner.with_space(&space.id, |conn| {
            let content: Value = serde_json::from_str(&pages::get_page(conn,&inner.session,&page.id)?.content).unwrap();
            assert_ne!(content.pointer("/content/0/content/0/attrs/nodeId").unwrap(),"old-node");
            assert!(content.pointer("/content/0/content/0/attrs/taskId").unwrap().is_null());
            let new_id = content.pointer("/content/1/attrs/attachmentId").unwrap().as_str().unwrap();
            assert_ne!(new_id,attachment_id);
            assert_eq!(notes::base64_decode(&attachments::open_attachment(&inner.files_dir(&space.id),conn,&inner.session,new_id)?.data_base64)?,b"owned resource");
            pages::save_page(conn,&inner.session,&page.id,"Edited","changed")
        }).unwrap();
        let second = instantiate(inner,&space.id,&section,None,"Second",&id,"global",None).unwrap();
        assert!(inner.with_space(&space.id, |conn| Ok(pages::get_page(conn,&inner.session,&second.id)?.content.contains("Checklist"))).unwrap());
        edit(inner,&space.id,&section,&id,"global",Some("Renamed")).unwrap();
        edit(inner,&space.id,&section,&id,"global",None).unwrap();
        assert!(list(inner,&space.id,&section).unwrap().is_empty());
        assert!(inner.with_space(&space.id, |conn| pages::get_page(conn,&inner.session,&second.id)).is_ok());
    }

    #[test]
    fn encrypted_templates_require_unlock_and_confirmation_and_block_password_removal() {
        let directory = tempfile::tempdir().unwrap();
        let root = layout::data_root(directory.path());
        let app = AppState::init(root.clone(), startup::startup(&root).unwrap()).unwrap();
        let inner = &app.inner;
        let space = inner.with_meta(registry::list_spaces).unwrap().remove(0);
        let (section, page) = inner.with_space(&space.id, |conn| {
            let notebook = hierarchy::create_notebook(conn,"N",None)?;
            let section = hierarchy::create_section(conn,&notebook.id,None,"S",None)?;
            let page = hierarchy::create_page(conn,&section.id,None,"Page")?;
            pages::save_page(conn,&inner.session,&page.id,"Page",r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"secret-text"}]}]}"#)?;
            let keys = sections_crypto::set_password(conn,&section.id,"password",true)?;
            inner.session.insert(&section.id,keys);
            Ok((section.id,page.id))
        }).unwrap();
        assert!(save(inner,&space.id,&page,"Secret",true,false).is_err());
        assert!(list(inner,&space.id,&section).unwrap().is_empty());
        let id = save(inner,&space.id,&page,"Secret",false,false).unwrap();
        let before = inner.with_space(&space.id, |conn| {
            let bytes: Vec<u8> = conn.query_row("SELECT content_ciphertext FROM section_templates WHERE id=?1",[&id],|row|row.get(0))?;
            assert!(!String::from_utf8_lossy(&bytes).contains("secret-text"));
            Ok(bytes)
        }).unwrap();
        assert!(inner.with_space(&space.id, |conn| sections_crypto::remove_password(conn,&inner.session,&section,"password")).is_err());
        inner.session.lock(&section);
        assert!(list(inner,&space.id,&section).unwrap().is_empty());
        assert!(instantiate(inner,&space.id,&section,None,"Copy",&id,"section",None).is_err());
        assert!(edit(inner,&space.id,&section,&id,"section",None).is_err());
        inner.with_space(&space.id, |conn| { inner.session.insert(&section,sections_crypto::unlock_keys(conn,&section,"password")?); Ok(()) }).unwrap();
        assert!(export_global(inner,&space.id,&section,&id,"Exported",false).is_err());
        let global = export_global(inner,&space.id,&section,&id,"Exported",true).unwrap();
        let copy = instantiate(inner,&space.id,&section,None,"Copy",&id,"section",None).unwrap();
        inner.with_space(&space.id, |conn| {
            let after: Vec<u8> = conn.query_row("SELECT content_ciphertext FROM section_templates WHERE id=?1",[&id],|row|row.get(0))?;
            assert_eq!(before,after);
            assert!(pages::get_page(conn,&inner.session,&copy.id)?.content.contains("secret-text"));
            Ok(())
        }).unwrap();
        edit(inner,&space.id,&section,&id,"section",Some("New name")).unwrap();
        edit(inner,&space.id,&section,&id,"section",None).unwrap();
        inner.with_space(&space.id, |conn| sections_crypto::remove_password(conn,&inner.session,&section,"password")).unwrap();
        assert!(instantiate(inner,&space.id,&section,None,"Plain",&global,"global",None).is_ok());
    }
}
