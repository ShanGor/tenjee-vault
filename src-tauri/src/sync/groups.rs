//! Atomic domain membership revisions and complete ciphertext conflict variants.
use super::storage::{self, Context, Record, Table};
use crate::error::{VaultError, VaultResult};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
pub struct Domain {
    pub id: String,
    pub context: Context,
    pub protected: bool,
    pub key_identity: String,
    pub records: Vec<Record>,
}

fn encode<T: Serialize>(value: &T) -> VaultResult<String> {
    serde_json::to_string(value).map_err(|_| VaultError::Validation("Invalid dependency group".into()))
}

fn domain_sql(table:&Table,prefix:&str)->String {
    let field=|name:&str|if prefix.is_empty(){format!("\"{}\".\"{name}\"",table.name)}else{format!("{prefix}{name}")};
    let section=match table.name.as_str(){
        "sections"=>field("id"),"pages"|"section_templates"=>field("section_id"),
        "page_versions"=>format!("(SELECT section_id FROM pages WHERE id={})",field("page_id")),
        "attachments"|"taggings"=>format!("(SELECT section_id FROM pages WHERE id={})",field("entity_id")),_=>return "'ordinary'".into(),
    };
    format!("COALESCE((SELECT 'domain:' || COALESCE(root_page_id,id) FROM sections WHERE id={section}),'ordinary')")
}

pub fn install(conn: &mut Connection) -> VaultResult<()> {
    let supported: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sync_groups')", [], |r| r.get(0))?;
    if !supported { return Ok(()); }
    conn.create_scalar_function("sync_context_union", 2, rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC, |args| {
        let left: String = args.get(0)?; let right: String = args.get(1)?;
        let left: Context = serde_json::from_str(&left).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
        let right: Context = serde_json::from_str(&right).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
        serde_json::to_string(&storage::union(&left, &right)).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))
    })?;
    let space: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sections')", [], |r| r.get(0))?;
    if !space { return Ok(()); }
    let tx = conn.transaction()?;
    // Private domain labels duplicate the protected root title. Keep no old
    // plaintext title in this internal storage label.
    tx.execute("UPDATE sections SET name='Protected page' WHERE is_encrypted=1 AND root_page_id IS NOT NULL AND name!='Protected page'", [])?;
    for table in storage::tables(&tx)? {
        let name = &table.name;
        let key = storage::key_sql(&table, "");
        let domain = domain_sql(&table, "");
        tx.execute_batch(&format!("INSERT OR REPLACE INTO sync_members(entity,entity_key,group_id) SELECT '{name}',{key},{domain} FROM \"{name}\";"))?;
        for (operation, prefix) in [("INSERT","NEW."),("UPDATE","NEW."),("DELETE","OLD.")] {
            let key = storage::key_sql(&table, prefix);
            let mapped = domain_sql(&table, prefix);
            let old = format!("COALESCE((SELECT group_id FROM sync_members WHERE entity='{name}' AND entity_key={key}),{mapped})");
            let current = if operation == "DELETE" { old.clone() } else { mapped };
            let guard = if operation == "UPDATE" { format!(" AND {} IS NOT {}", storage::payload_sql(&table,"OLD."),storage::payload_sql(&table,"NEW.")) } else { String::new() };
            let touch = |group: &str| format!("UPDATE sync_state SET sequence=sequence+1;
                INSERT INTO sync_groups(group_id,context) VALUES({group},json_object((SELECT origin FROM sync_state),(SELECT sequence FROM sync_state)))
                ON CONFLICT(group_id) DO UPDATE SET context=sync_context_union(context,excluded.context);");
            tx.execute_batch(&format!("DROP TRIGGER IF EXISTS sync_group_{name}_{operation}; CREATE TRIGGER sync_group_{name}_{operation} AFTER {operation} ON \"{name}\"
                WHEN (SELECT importing FROM sync_state)=0{guard}
                BEGIN
                  SELECT RAISE(ABORT,'Resolve the protected domain conflict before editing') WHERE EXISTS(SELECT 1 FROM sync_domain_variants WHERE group_id IN ({old},{current}));
                  {}
                  {}
                  INSERT INTO sync_members(entity,entity_key,group_id) VALUES('{name}',{key},{current}) ON CONFLICT(entity,entity_key) DO UPDATE SET group_id=excluded.group_id;
                END;",touch(&old),touch(&current)))?;
        }
    }
    for record in storage::inventory(&tx)? {
        let group: String = tx.query_row("SELECT COALESCE((SELECT group_id FROM sync_members WHERE entity=?1 AND entity_key=?2),'ordinary')", params![record.entity,record.key], |r| r.get(0))?;
        tx.execute("INSERT INTO sync_groups(group_id,context) VALUES(?1,?2) ON CONFLICT(group_id) DO UPDATE SET context=sync_context_union(context,excluded.context)",params![group,encode(&record.context)?])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn inventory(conn: &Connection, records: &[Record]) -> VaultResult<Vec<Domain>> {
    let mut domains: BTreeMap<String, Domain> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT group_id,context FROM sync_groups ORDER BY group_id")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))? {
        let (id,context) = row?;
        let context = serde_json::from_str(&context).map_err(|_| VaultError::Validation("Invalid domain revision".into()))?;
        domains.insert(id.clone(), Domain{id,context,protected:false,key_identity:String::new(),records:Vec::new()});
    }
    for record in records {
        let group: String = conn.query_row("SELECT COALESCE((SELECT group_id FROM sync_members WHERE entity=?1 AND entity_key=?2),'ordinary')",params![record.entity,record.key],|r|r.get(0))?;
        let domain = domains.entry(group.clone()).or_insert_with(|| Domain{id:group,context:Context::new(),protected:false,key_identity:String::new(),records:Vec::new()});
        domain.context = storage::union(&domain.context,&record.context);
        if record.entity == "sections" && !record.deleted {
            let payload = record.payload.as_ref().unwrap();
            domain.protected = payload["is_encrypted"].as_i64() == Some(1);
            let key = serde_json::json!([payload["is_encrypted"],payload["kdf_salt"],payload["kdf_params"],payload["verifier"],payload["wrapped_dsk"]]);
            domain.key_identity = crate::blob_store::hash_hex(encode(&key)?.as_bytes());
        }
        domain.records.push(record.clone());
    }
    Ok(domains.into_values().collect())
}

pub fn save_context(conn: &Connection, id: &str, context: &Context) -> VaultResult<()> {
    conn.execute("INSERT INTO sync_groups(group_id,context) VALUES(?1,?2) ON CONFLICT(group_id) DO UPDATE SET context=sync_context_union(context,excluded.context)",params![id,encode(context)?])?;
    Ok(())
}

pub fn retain_variant(conn: &Connection, domain: &Domain) -> VaultResult<()> {
    // Only complete protected alternatives belong here. Ordinary/protected
    // races are deferred using metadata before bodies are requested.
    if !domain.protected { return Err(VaultError::Validation("Plaintext domain alternatives must remain pending at their source".into())); }
    let payload = encode(domain)?;
    let id = crate::blob_store::hash_hex(payload.as_bytes());
    conn.execute("INSERT OR IGNORE INTO sync_domain_variants(group_id,variant_id,context,payload) VALUES(?1,?2,?3,?4)",params![domain.id,id,encode(&domain.context)?,payload])?;
    Ok(())
}

/// Incoming writes suppress local revisions; rebuild membership explicitly.
pub fn refresh_members(conn:&Connection)->VaultResult<()> {
    let space:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='pages')",[],|r|r.get(0))?;
    if !space {return Ok(());}
    for table in storage::tables(conn)? {
        let name=&table.name;let key=storage::key_sql(&table,"");let domain=domain_sql(&table,"");
        conn.execute_batch(&format!("INSERT INTO sync_members(entity,entity_key,group_id) SELECT '{name}',{key},{domain} FROM \"{name}\" WHERE true ON CONFLICT(entity,entity_key) DO UPDATE SET group_id=excluded.group_id;"))?;
    }Ok(())
}
