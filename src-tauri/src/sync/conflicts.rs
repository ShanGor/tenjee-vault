//! Local conflict decisions; peer pairing never grants plaintext access.
use super::{storage,groups,session::ExchangeManager};
use crate::{commands::{AppState,AppStateInner},error::{VaultError,VaultResult}};
use rusqlite::{Connection,params};
use serde::Serialize;
use std::collections::{BTreeMap,BTreeSet};
use tauri::{Manager,State};

#[derive(Serialize)]
pub struct Variant {pub id:String,pub label:String,pub deleted:bool,pub protected:bool,pub preview:Option<String>}
#[derive(Serialize)]
pub struct Conflict {pub id:String,pub store:String,pub entity:String,pub key:String,pub label:String,pub variants:Vec<Variant>}
fn invalid(text:&str)->VaultError {VaultError::Validation(text.into())}
fn with_store<R>(inner:&AppStateInner,store:&str,work:impl FnOnce(&mut Connection)->VaultResult<R>)->VaultResult<R>{
    inner.with_workspace(|meta,tasks,calendar,spaces|match store {
        "meta"=>work(meta),"tasks"=>work(tasks),"calendar"=>work(calendar),
        id if id.starts_with("space:") && uuid::Uuid::parse_str(&id[6..]).is_ok()=>work(spaces.get_mut(&id[6..]).ok_or_else(||invalid("Space is unavailable"))?),
        _=>Err(invalid("Invalid conflict store")),
    })
}
fn label(record:&storage::Record,protected:bool)->String {
    if record.deleted {return "Deleted variant".into();}
    if protected {return "Protected ciphertext variant".into();}
    record.payload.as_ref().and_then(|p|p.get("title").or_else(||p.get("name"))).and_then(|v|v.as_str()).map(|s|s.chars().take(120).collect()).unwrap_or_else(||record.entity.clone())
}
#[tauri::command]
pub fn exchange_conflicts_cmd(state:State<'_,AppState>)->VaultResult<Vec<Conflict>> {
    state.inner.with_workspace(|meta,tasks,calendar,spaces|{
        let mut result=Vec::new();
        let mut collect=|store:String,conn:&Connection|->VaultResult<()> {
            let domains=if store.starts_with("space:"){groups::inventory(conn,&storage::inventory(conn)?)?}else{Vec::new()};
            let protected=domains.iter().filter(|d|d.protected).flat_map(|d|d.records.iter().map(|r|(&r.entity,&r.key))).collect::<BTreeSet<_>>();
            let mut by_entity:BTreeMap<(String,String),Vec<storage::Record>>=BTreeMap::new();
            for record in storage::conflicts(conn)? {by_entity.entry((record.entity.clone(),record.key.clone())).or_default().push(record);}
            for ((entity,key),records) in by_entity {
                let private=protected.contains(&(&entity,&key)) || records.iter().any(|r|r.entity=="pages" && r.payload.as_ref().is_some_and(|p|p["title_is_encrypted"].as_i64()==Some(1)));
                let id=crate::blob_store::hash_hex(format!("{store}:{entity}:{key}").as_bytes());
                result.push(Conflict{id,store:store.clone(),label:entity.clone(),entity,key,variants:records.iter().map(|r|Ok(Variant{id:storage::variant_id(r)?,label:label(r,private),deleted:r.deleted,protected:private,preview:if private{None}else{preview(r)}})).collect::<VaultResult<Vec<_>>>()?});
            }
            let mut stmt=conn.prepare("SELECT group_id,variant_id,payload FROM sync_domain_variants ORDER BY group_id,variant_id")?;
            let mut groups:BTreeMap<String,Vec<Variant>>=BTreeMap::new();
            for row in stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))? {
                let (group,id,payload)=row?;let domain:groups::Domain=serde_json::from_str(&payload).map_err(|_|invalid("Invalid stored protected variant"))?;
                groups.entry(group).or_default().push(Variant{id,label:format!("{} · {}",domain.records.len(),&domain.key_identity[..domain.key_identity.len().min(12)]),deleted:false,protected:true,preview:None});
            }
            for (key,variants) in groups {result.push(Conflict{id:crate::blob_store::hash_hex(format!("{store}:{key}").as_bytes()),store:store.clone(),entity:"protected-domain".into(),label:"Protected tree".into(),key,variants});}
            Ok(())
        };
        collect("meta".into(),meta)?;collect("tasks".into(),tasks)?;collect("calendar".into(),calendar)?;
        let mut ids=spaces.keys().cloned().collect::<Vec<_>>();ids.sort();for id in ids{collect(format!("space:{id}"),&spaces[&id])?;}
        Ok(result)
    })
}
#[tauri::command]
pub fn exchange_resolve_cmd(app:tauri::AppHandle,state:State<'_,AppState>,store:String,entity:String,key:String,variant:String)->VaultResult<()> {
    app.state::<ExchangeManager>().stop();
    with_store(&state.inner,&store,|conn|{
        if entity=="protected-domain" {resolve_domain(conn,&state.inner,&key,&variant)?;}else{storage::resolve(conn,&entity,&key,&variant)?;groups::refresh_members(conn)?;}
        if store.starts_with("space:"){state.inner.session.lock_all();crate::commands::notes::rebuild_indexes(&state.inner,conn)?;}
        Ok(())
    })?;
    super::engine::recover(&state.inner)?;Ok(())
}
fn resolve_domain(conn:&mut Connection,inner:&AppStateInner,key:&str,variant:&str)->VaultResult<()> {
    let mut stmt=conn.prepare("SELECT variant_id,payload FROM sync_domain_variants WHERE group_id=?1 ORDER BY variant_id")?;
    let rows=stmt.query_map([key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<Result<Vec<_>,_>>()?;drop(stmt);
    let mut context=storage::Context::new();let mut all=BTreeMap::new();let mut chosen=None;
    for (id,payload) in rows {
        let domain:groups::Domain=serde_json::from_str(&payload).map_err(|_|invalid("Invalid stored protected variant"))?;
        context=storage::union(&context,&domain.context);
        for record in &domain.records {context=storage::union(&context,&record.context);all.insert((record.entity.clone(),record.key.clone()),record.clone());}
        if id==variant {chosen=Some(domain);}
    }
    let chosen=chosen.ok_or_else(||invalid("Protected variant is no longer available"))?;
    let mut selected=BTreeMap::new();for record in chosen.records {
        if selected.insert((record.entity.clone(),record.key.clone()),record).is_some(){return Err(invalid("Resolve member conflicts before selecting this protected domain"));}
    }
    let tx=conn.savepoint()?;tx.execute_batch("PRAGMA defer_foreign_keys=ON; UPDATE sync_state SET importing=1;")?;
    let (origin,sequence):(String,u64)=tx.query_row("UPDATE sync_state SET sequence=sequence+1 RETURNING origin,sequence",[],|r|Ok((r.get(0)?,r.get(1)?)))?;context.insert(origin.clone(),sequence);
    tx.execute("DELETE FROM sync_domain_variants WHERE group_id=?1",[key])?;
    for ((entity,identity),mut record) in all {
        if let Some(value)=selected.remove(&(entity.clone(),identity.clone())){record=value;}else{record.deleted=true;record.payload=None;}
        record.context=context.clone();storage::materialize(&tx,&record)?;storage::save_head(&tx,&record)?;
        tx.execute("DELETE FROM sync_conflicts WHERE entity=?1 AND entity_key=?2",params![entity,identity])?;
        tx.execute("INSERT INTO sync_changes(origin,sequence,entity,entity_key) VALUES(?1,?2,?3,?4)",params![origin,sequence,record.entity,record.key])?;
        tx.execute("INSERT INTO sync_members(entity,entity_key,group_id) VALUES(?1,?2,?3) ON CONFLICT(entity,entity_key) DO UPDATE SET group_id=excluded.group_id",params![record.entity,record.key,key])?;
    }
    groups::save_context(&tx,key,&context)?;storage::validate_hierarchy(&tx)?;tx.execute("UPDATE sync_state SET importing=0",[])?;tx.commit()?;
    inner.session.lock_all();Ok(())
}

/// Duplicate only references owned by the selected entity/subtree. Unrelated
/// incoming links keep their original IDs.
#[tauri::command]
pub fn exchange_keep_both_cmd(app:tauri::AppHandle,state:State<'_,AppState>,store:String,entity:String,key:String,variant:String,password:Option<String>)->VaultResult<()> {
    app.state::<ExchangeManager>().stop();
    let password=password.map(zeroize::Zeroizing::new);
    with_store(&state.inner,&store,|conn| {
        conn.execute_batch("SAVEPOINT keep_both;")?;
        let outcome=(|| {

        let local_session=crate::notes::session::SessionManager::new();
        let (selected,original,records)=if entity=="protected-domain" {
            let payload:String=conn.query_row("SELECT payload FROM sync_domain_variants WHERE group_id=?1 AND variant_id=?2",params![key,variant],|r|r.get(0))?;
            let selected:groups::Domain=serde_json::from_str(&payload).map_err(|_|invalid("Invalid protected variant"))?;
            let current=groups::inventory(conn,&storage::inventory(conn)?)?.into_iter().find(|d|d.id==key).ok_or_else(||invalid("Current protected tree is missing"))?;
            let original=crate::blob_store::hash_hex(serde_json::to_string(&current).map_err(|_|invalid("Invalid domain"))?.as_bytes());
            let wrapper=selected.records.iter().find(|r|r.entity=="sections" && !r.deleted).ok_or_else(||invalid("Wrapping metadata missing"))?;
            let root=wrapper.payload.as_ref().and_then(|p|p["root_page_id"].as_str()).ok_or_else(||invalid("Protected root is missing"))?;
            unlock_variant(&local_session,wrapper,password.as_deref().map(|s|s.as_str()).ok_or_else(||invalid("Enter the selected protected variant's password locally"))?)?;
            let selected_root=selected.records.iter().find(|r|r.entity=="pages" && r.payload.as_ref().is_some_and(|p|p["id"]==root)).cloned().ok_or_else(||invalid("Protected root payload is missing"))?;
            (selected_root,original,selected.records)
        } else {
            let heads=storage::conflicts(conn)?.into_iter().filter(|r|r.entity==entity && r.key==key).collect::<Vec<_>>();
            let selected=heads.iter().find(|r|storage::variant_id(r).ok().as_deref()==Some(&variant)).cloned().ok_or_else(||invalid("Variant is no longer available"))?;
            let current=storage::get(conn,&entity,&key)?.ok_or_else(||invalid("Original entity is missing"))?;
            let original=storage::variant_id(&current)?;
            let records=copy_members(conn,&selected)?;
            for wrapper in records.iter().filter(|r|r.entity=="sections" && r.payload.as_ref().is_some_and(|p|p["is_encrypted"].as_i64()==Some(1))) {
                if let Some(password)=password.as_deref(){unlock_variant(&local_session,wrapper,password.as_str())?;}
            }
            (selected,original,records)
        };
        if selected.deleted{return Err(invalid("A deletion cannot be duplicated; select an existing variant"));}
        let session=if password.is_some(){&local_session}else{&state.inner.session};
        let tx=conn.savepoint()?;tx.execute_batch("PRAGMA defer_foreign_keys=ON; UPDATE sync_state SET importing=1;")?;
        duplicate_records(&tx,session,&selected,&records)?;
        tx.execute("UPDATE sync_state SET importing=0",[])?;tx.commit()?;
        if entity=="protected-domain" {resolve_domain(conn,&state.inner,&key,&original)?;}else{storage::resolve(conn,&entity,&key,&original)?;}
        groups::refresh_members(conn)?;Ok(())
        })();
        match outcome {Ok(())=>{
            let broken:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",[],|r|r.get(0))?;
            if broken{conn.execute_batch("ROLLBACK TO keep_both; RELEASE keep_both;")?;return Err(invalid("Duplicate dependency references are incomplete"));}
            conn.execute_batch("RELEASE keep_both;")?;Ok(())},Err(error)=>{conn.execute_batch("ROLLBACK TO keep_both; RELEASE keep_both;")?;Err(error)}}
    })?;
    super::engine::recover(&state.inner)?;Ok(())
}
fn unlock_variant(session:&crate::notes::session::SessionManager,record:&storage::Record,password:&str)->VaultResult<()> {
    let payload=record.payload.as_ref().ok_or_else(||invalid("Missing wrapping payload"))?;
    fn bytes(value:&serde_json::Value)->VaultResult<Vec<u8>> {
        let hex=value.get("$blob").and_then(|v|v.as_str()).ok_or_else(||invalid("Missing wrapped key bytes"))?;
        if hex.len()%2!=0 || !hex.bytes().all(|b|b.is_ascii_hexdigit()){return Err(invalid("Invalid wrapped key bytes"));}
        Ok(hex.as_bytes().chunks(2).map(|p|u8::from_str_radix(std::str::from_utf8(p).unwrap(),16).unwrap()).collect())
    }
    let parameters:crate::crypto::kdf::KdfParams=serde_json::from_str(payload["kdf_params"].as_str().ok_or_else(||invalid("Missing KDF parameters"))?).map_err(|_|invalid("Invalid KDF parameters"))?;
    let wrapped=crate::crypto::keys::WrappedDsk{salt:bytes(&payload["kdf_salt"])?,m_cost:parameters.m_cost,t_cost:parameters.t_cost,p_cost:parameters.p_cost,verifier:bytes(&payload["verifier"])?,wrapped_dsk:bytes(&payload["wrapped_dsk"])?};
    session.insert(payload["id"].as_str().ok_or_else(||invalid("Invalid domain ID"))?,crate::crypto::keys::unwrap_dsk(&wrapped,password)?);Ok(())
}
fn copy_members(conn:&Connection,selected:&storage::Record)->VaultResult<Vec<storage::Record>> {
    let id=selected.payload.as_ref().and_then(|p|p["id"].as_str()).ok_or_else(||invalid("This entity cannot be duplicated"))?;
    let all=storage::inventory(conn)?;
    let mut ids=BTreeSet::from([id.to_string()]);
    if selected.entity=="pages" || selected.entity=="tasks" {
        let parent=if selected.entity=="pages"{"parent_page_id"}else{"parent_task_id"};
        loop {let old=ids.len();for record in all.iter().filter(|r|r.entity==selected.entity && !r.deleted){if let Some(payload)=&record.payload{if payload[parent].as_str().is_some_and(|id|ids.contains(id)){if let Some(id)=payload["id"].as_str(){ids.insert(id.into());}}}}if old==ids.len(){break;}}
    }
    let domains=all.iter().filter(|r|r.entity=="pages" && r.payload.as_ref().is_some_and(|p|p["id"].as_str().is_some_and(|id|ids.contains(id)))).filter_map(|r|r.payload.as_ref()?.get("section_id")?.as_str()).collect::<BTreeSet<_>>();
    let mut records=Vec::new();
    for record in all.iter().filter(|r|!r.deleted) {
        let payload=record.payload.as_ref().unwrap();
        let owned=match record.entity.as_str(){
            name if name==selected.entity=>payload["id"].as_str().is_some_and(|id|ids.contains(id)),
            "page_versions" if selected.entity=="pages"=>payload["page_id"].as_str().is_some_and(|id|ids.contains(id)),
            "attachments"|"taggings"=>payload["entity_id"].as_str().is_some_and(|id|ids.contains(id)) && payload["entity_type"]==if selected.entity=="pages"{"page"}else if selected.entity=="tasks"{"task"}else{"__none"},
            "event_exceptions"|"event_reminders" if selected.entity=="events"=>payload["event_id"]==id,
            "sections" if selected.entity=="pages"=>payload["id"].as_str().is_some_and(|id|domains.contains(id)) && payload["is_encrypted"].as_i64()==Some(1) && payload["root_page_id"].as_str().is_some_and(|id|ids.contains(id)),
            "section_templates" if selected.entity=="pages"=>payload["section_id"].as_str().is_some_and(|id|domains.contains(id)) && all.iter().any(|r|r.entity=="sections" && r.payload.as_ref().is_some_and(|p|p["id"]==payload["section_id"] && p["root_page_id"].as_str().is_some_and(|id|ids.contains(id)))),
            _=>false,
        };
        if owned{records.push(if record.entity==selected.entity && record.key==selected.key{selected.clone()}else{record.clone()});}
    }
    if !records.iter().any(|r|r.entity==selected.entity && r.key==selected.key){records.push(selected.clone());}
    Ok(records)
}
fn duplicate_records(conn:&Connection,session:&crate::notes::session::SessionManager,selected:&storage::Record,records:&[storage::Record])->VaultResult<()> {
    let mut ids=BTreeMap::new();let mut identities=BTreeSet::new();let mut context=storage::Context::new();
    for record in records.iter().filter(|r|!r.deleted){
        if !identities.insert((&record.entity,&record.key)){return Err(invalid("Resolve member conflicts before duplicating this subtree"));}
        context=storage::union(&context,&record.context);
        if let Some(id)=record.payload.as_ref().and_then(|p|p["id"].as_str()){ids.insert(id.to_string(),uuid::Uuid::new_v4().to_string());}
    }
    let (origin,sequence):(String,u64)=conn.query_row("UPDATE sync_state SET sequence=sequence+1 RETURNING origin,sequence",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    context.insert(origin.clone(),sequence);
    let section_flags=records.iter().filter(|r|r.entity=="sections").filter_map(|r|Some((r.payload.as_ref()?.get("id")?.as_str()?.to_string(),r.payload.as_ref()?.get("is_encrypted")?.as_i64()==Some(1)))).collect::<BTreeMap<_,_>>();
    let mut copies=Vec::new();
    for original in records.iter().filter(|r|!r.deleted) {
        let mut record=original.clone();let payload=record.payload.as_mut().unwrap().as_object_mut().unwrap();
        let domain=payload.get("section_id").and_then(|v|v.as_str()).map(str::to_string);
        if matches!(record.entity.as_str(),"pages"|"page_versions") {
            let domain=if let Some(domain)=domain.clone(){domain}else{let page=payload["page_id"].as_str().ok_or_else(||invalid("Missing version page"))?;records.iter().find(|r|r.entity=="pages" && r.payload.as_ref().is_some_and(|p|p["id"]==page)).and_then(|r|r.payload.as_ref()?.get("section_id")?.as_str()).ok_or_else(||invalid("Missing version domain"))?.into()};
            let encrypted=if let Some(flag)=section_flags.get(&domain){*flag}else{crate::notes::section_encrypted(conn,&domain)?};
            let content=payload["content"].as_str().ok_or_else(||invalid("Missing page content"))?;
            let plain=crate::notes::reveal_content(session,&domain,encrypted,content)?;
            let remapped=remap_document(&plain,&ids);
            payload.insert("content".into(),serde_json::json!(crate::notes::protect_content(session,&domain,encrypted,&remapped)?));
        }
        if record.entity=="section_templates" {
            let domain=domain.as_deref().ok_or_else(||invalid("Missing template domain"))?;
            let value=&payload["content_ciphertext"];let hex=value.get("$blob").and_then(|v|v.as_str()).ok_or_else(||invalid("Invalid template ciphertext"))?;
            let bytes=hex.as_bytes().chunks(2).map(|p|u8::from_str_radix(std::str::from_utf8(p).unwrap_or(""),16).map_err(|_|invalid("Invalid template encoding"))).collect::<VaultResult<Vec<_>>>()?;
            let plain=crate::notes::reveal_bytes(session,domain,true,&bytes)?;
            let text=String::from_utf8(plain).map_err(|_|invalid("Invalid template text"))?;
            let encrypted=crate::notes::protect_bytes(session,domain,true,remap_document(&text,&ids).as_bytes())?;
            payload.insert("content_ciphertext".into(),serde_json::json!({"$blob":encrypted.iter().map(|b|format!("{b:02X}")).collect::<String>()}));
        }
        for field in ["id","parent_page_id","page_id","entity_id","section_id","root_page_id","parent_task_id","event_id"] {
            if let Some(value)=payload.get(field).and_then(|v|v.as_str()).and_then(|id|ids.get(id)).cloned(){payload.insert(field.into(),serde_json::json!(value));}
        }
        if original.entity==selected.entity && original.key==selected.key {
            if let Some(title)=payload.get("title").and_then(|v|v.as_str()).map(str::to_string){
                let encrypted=payload.get("title_is_encrypted").and_then(|v|v.as_i64())==Some(1);
                let domain=domain.as_deref().unwrap_or("");let title=crate::notes::reveal_content(session,domain,encrypted,&title)?;
                payload.insert("title".into(),serde_json::json!(format!("{title} (copy)")));
                if record.entity == "pages" { payload.insert("title_is_encrypted".into(),serde_json::json!(0)); }
            } else if let Some(name)=payload.get("name").and_then(|v|v.as_str()).map(str::to_string){payload.insert("name".into(),serde_json::json!(format!("{name} (copy {})",&uuid::Uuid::new_v4().to_string()[..8])));}
        }
        let table=storage::tables(conn)?.into_iter().find(|t|t.name==record.entity).ok_or_else(||invalid("Unsupported duplicate type"))?;
        record.key=serde_json::to_string(&table.keys.iter().map(|key|payload[key].clone()).collect::<Vec<_>>()).map_err(|_|invalid("Cannot encode duplicate ID"))?;
        record.context=context.clone();copies.push(record);
    }
    copies.sort_by_key(|r|match r.entity.as_str(){"notebooks"|"task_lists"=>0,"sections"=>1,"pages"|"tasks"|"events"=>2,_=>3});
    for record in copies {
        storage::materialize(conn,&record)?;storage::save_head(conn,&record)?;
        conn.execute("INSERT INTO sync_changes(origin,sequence,entity,entity_key) VALUES(?1,?2,?3,?4)",params![origin,sequence,record.entity,record.key])?;
    }
    storage::validate_hierarchy(conn)?;Ok(())
}
fn remap_document(content:&str,ids:&BTreeMap<String,String>)->String {
    let Ok(mut document)=serde_json::from_str::<serde_json::Value>(content) else {return content.into();};
    fn walk(value:&mut serde_json::Value,ids:&BTreeMap<String,String>){
        match value {
            serde_json::Value::Object(object)=>{for (key,value) in object.iter_mut(){if matches!(key.as_str(),"pageId"|"taskId"|"attachmentId"|"href"|"src"){if let Some(text)=value.as_str(){let mut mapped=text.to_string();if let Some(id)=ids.get(text){mapped=id.clone();}else{for (from,to) in ids{for marker in ["/page/","attachment:","page:"] {mapped=mapped.replace(&format!("{marker}{from}"),&format!("{marker}{to}"));}}}*value=serde_json::json!(mapped);}}else{walk(value,ids);}}},
            serde_json::Value::Array(array)=>{for child in array{walk(child,ids);}},_=>{}
        }
    }
    walk(&mut document,ids);document.to_string()
}

fn preview(record:&storage::Record)->Option<String> {
    let payload=record.payload.as_ref()?;
    let text=payload.get("content").or_else(||payload.get("notes")).or_else(||payload.get("description")).and_then(|v|v.as_str());
    fn plain(value:&serde_json::Value,out:&mut String){
        if out.len()>2000{return;}
        if let Some(text)=value.get("text").and_then(|v|v.as_str()){out.push_str(text);out.push(' ');}
        if let Some(children)=value.get("content").and_then(|v|v.as_array()){for child in children{plain(child,out);}}
    }
    let mut snippet=String::new();
    if let Some(text)=text {if let Ok(document)=serde_json::from_str::<serde_json::Value>(text){plain(&document,&mut snippet);}else{snippet=text.chars().take(300).collect();}}
    let status=payload.get("status").and_then(|v|v.as_str()).unwrap_or("");
    let date=payload.get("updated_at").or_else(||payload.get("start_at")).and_then(|v|v.as_str()).unwrap_or("");
    Some(format!("{} · {status} · {date}",snippet.chars().take(300).collect::<String>()))
}
