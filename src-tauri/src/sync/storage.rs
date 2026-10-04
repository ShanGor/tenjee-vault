use crate::error::{VaultError, VaultResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub type Context = BTreeMap<String, u64>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub entity: String,
    pub key: String,
    pub context: Context,
    pub payload: Option<Value>,
    pub deleted: bool,
}

#[derive(Clone, Debug)]
pub struct Table {
    pub name: String,
    pub columns: Vec<String>,
    pub keys: Vec<String>,
}

fn invalid(message: impl Into<String>) -> VaultError {
    VaultError::Validation(message.into())
}
fn encode(value: &impl Serialize) -> VaultResult<String> {
    serde_json::to_string(value).map_err(|_| invalid("Cannot encode sync record"))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> VaultResult<T> {
    serde_json::from_str(value).map_err(|_| invalid("Invalid sync record"))
}

pub fn installation_origin(root: &std::path::Path) -> VaultResult<String> {
    use std::io::Write;
    let parent = root
        .parent()
        .ok_or_else(|| invalid("Missing installation directory"))?;
    // Lives outside the backed-up vault: restoring/cloning data cannot clone
    // the writer identity. The value is a non-secret random replica identifier.
    let path = parent.join(".tenjee-sync-origin");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            let origin = uuid::Uuid::new_v4().to_string();
            file.write_all(origin.as_bytes())?;
            file.sync_all()?;
            Ok(origin)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let origin = std::fs::read_to_string(path)?;
            uuid::Uuid::parse_str(&origin)
                .map_err(|_| invalid("Invalid installation replica identity"))?;
            Ok(origin)
        }
        Err(error) => Err(error.into()),
    }
}

pub fn rotate_installation_origin(root: &std::path::Path) -> VaultResult<()> {
    let parent = root
        .parent()
        .ok_or_else(|| invalid("Missing installation directory"))?;
    let temp = parent.join(format!(".tenjee-origin-{}", uuid::Uuid::new_v4()));
    std::fs::write(&temp, uuid::Uuid::new_v4().to_string())?;
    std::fs::rename(temp, parent.join(".tenjee-sync-origin"))?;
    Ok(())
}

pub fn tables(conn: &Connection) -> VaultResult<Vec<Table>> {
    // Deliberate allowlist: preferences, paths, FTS, queues and reminder delivery
    // history are never represented as replicated records.
    let names: &[&str] = if exists(conn, "spaces")? {
        &["spaces", "tags", "page_templates"]
    } else if exists(conn, "tasks")? {
        &["task_lists", "tasks", "attachments", "taggings"]
    } else if exists(conn, "events")? {
        &["events", "event_exceptions", "event_reminders", "taggings"]
    } else {
        &[
            "notebooks",
            "section_groups",
            "sections",
            "pages",
            "page_versions",
            "section_templates",
            "attachments",
            "taggings",
        ]
    };
    let mut result = Vec::new();
    for name in names {
        if !exists(conn, name)? {
            continue;
        }
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({name})"))?;
        let columns = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let keys = if *name == "taggings" {
            vec!["tag_id".into(), "entity_type".into(), "entity_id".into()]
        } else {
            let mut primary: Vec<_> = columns
                .iter()
                .filter(|(_, position)| *position > 0)
                .collect();
            primary.sort_by_key(|(_, position)| position);
            primary
                .into_iter()
                .map(|(column, _)| column.clone())
                .collect()
        };
        if keys.is_empty() {
            return Err(invalid(format!("Missing sync identity for {name}")));
        }
        result.push(Table {
            name: name.to_string(),
            columns: columns
                .into_iter()
                .map(|(column, _)| column)
                .filter(|column| *name != "spaces" || column != "db_file")
                .collect(),
            keys,
        });
    }
    Ok(result)
}

fn exists(conn: &Connection, name: &str) -> VaultResult<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |row| row.get(0),
    )?)
}

pub(crate) fn key_sql(table: &Table, prefix: &str) -> String {
    format!(
        "json_array({})",
        table
            .keys
            .iter()
            .map(|column| format!("{prefix}\"{column}\""))
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub(crate) fn payload_sql(table: &Table, prefix: &str) -> String {
    format!("json_object({})", table.columns.iter().flat_map(|column| {
        // JSON has no binary value. Tagged hex preserves encrypted BLOBs without
        // decrypting and cannot be interpreted as a file path or SQL fragment.
        let value = format!("{prefix}\"{column}\"");
        [format!("'{column}'"), format!("CASE WHEN typeof({value})='blob' THEN json_object('$blob',hex({value})) ELSE {value} END")]
    }).collect::<Vec<_>>().join(","))
}

pub fn install(conn: &mut Connection) -> VaultResult<()> {
    if !exists(conn, "sync_state")? {
        return Ok(());
    }
    let tx = conn.transaction()?;
    for table in tables(&tx)? {
        let name = &table.name;
        let baseline_key = key_sql(&table, "");
        let baseline_payload = payload_sql(&table, "");
        // A shared migrated backup retains these contexts. Actual subsequent
        // writers switch to their installation's origin on opening the database.
        tx.execute_batch(&format!("INSERT OR IGNORE INTO sync_objects(entity,entity_key,context,payload,deleted)
            SELECT '{name}',{baseline_key},json_object((SELECT origin FROM sync_state),1),{baseline_payload},0 FROM \"{name}\";
            UPDATE sync_state SET sequence=max(sequence,1);"))?;
        for (operation, prefix, deleted) in [
            ("INSERT", "NEW.", false),
            ("UPDATE", "NEW.", false),
            ("DELETE", "OLD.", true),
        ] {
            let key = key_sql(&table, prefix);
            let payload = if deleted {
                "NULL".to_string()
            } else {
                payload_sql(&table, prefix)
            };
            let guard = if operation == "UPDATE" {
                format!(" AND {} IS NOT {}", payload_sql(&table, "OLD."), payload)
            } else {
                String::new()
            };
            let context = format!("json_set(COALESCE((SELECT context FROM sync_objects WHERE entity='{name}' AND entity_key={key}),'{{}}'), '$.' || (SELECT origin FROM sync_state), (SELECT sequence FROM sync_state))");
            tx.execute_batch(&format!("DROP TRIGGER IF EXISTS sync_{name}_{operation}; CREATE TRIGGER sync_{name}_{operation} AFTER {operation} ON \"{name}\"
              WHEN (SELECT importing FROM sync_state)=0{guard}
              BEGIN
                SELECT RAISE(ABORT,'Resolve synchronization conflict before editing')
                  WHERE EXISTS(SELECT 1 FROM sync_conflicts WHERE entity='{name}' AND entity_key={key});
                UPDATE sync_state SET sequence=sequence+1;
                INSERT INTO sync_changes(origin,sequence,entity,entity_key) SELECT origin,sequence,'{name}',{key} FROM sync_state;
                INSERT INTO sync_objects(entity,entity_key,context,payload,deleted) VALUES('{name}',{key},{context},{payload},{})
                  ON CONFLICT(entity,entity_key) DO UPDATE SET context=excluded.context,payload=excluded.payload,deleted=excluded.deleted;
              END;", i32::from(deleted)))?;
        }
    }
    tx.commit()?;
    super::groups::install(conn)?;
    Ok(())
}

pub fn set_origin(conn: &Connection, origin: &str) -> VaultResult<()> {
    if origin.is_empty()
        || !origin
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || origin.len() > 80
    {
        return Err(invalid("Invalid replica origin"));
    }
    conn.execute("UPDATE sync_state SET origin=?1", [origin])?;
    Ok(())
}

pub fn inventory(conn: &Connection) -> VaultResult<Vec<Record>> {
    let mut stmt = conn.prepare("SELECT entity,entity_key,context,payload,deleted FROM sync_objects ORDER BY entity,entity_key")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(entity, key, context, payload, deleted)| {
            Ok(Record {
                entity,
                key,
                context: decode(&context)?,
                payload: payload.as_deref().map(decode).transpose()?,
                deleted,
            })
        })
        .collect()
}

pub fn dominates(left: &Context, right: &Context) -> bool {
    right
        .iter()
        .all(|(origin, sequence)| left.get(origin).copied().unwrap_or(0) >= *sequence)
}

pub fn union(left: &Context, right: &Context) -> Context {
    let mut result = left.clone();
    for (origin, sequence) in right {
        result
            .entry(origin.clone())
            .and_modify(|value| *value = (*value).max(*sequence))
            .or_insert(*sequence);
    }
    result
}

pub fn validate(conn: &Connection, record: &Record) -> VaultResult<Table> {
    let table = tables(conn)?
        .into_iter()
        .find(|table| table.name == record.entity)
        .ok_or_else(|| invalid("Unsupported synchronized entity"))?;
    let keys: Vec<Value> = decode(&record.key)?;
    if keys.len() != table.keys.len()
        || keys
            .iter()
            .any(|value| !value.is_string() && !value.is_number())
    {
        return Err(invalid("Invalid synchronized entity identity"));
    }
    if record.context.is_empty()
        || record.context.len() > 1024
        || record.context.iter().any(|(origin, sequence)| {
            origin.is_empty()
                || origin.len() > 80
                || !origin
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                || *sequence == 0
                || *sequence > i64::MAX as u64
        })
    {
        return Err(invalid("Invalid revision context"));
    }
    match (&record.payload, record.deleted) {
        (None, true) => (),
        (Some(Value::Object(payload)), false) => {
            if payload.len() != table.columns.len()
                || table
                    .columns
                    .iter()
                    .any(|column| !payload.contains_key(column))
            {
                return Err(invalid("Incompatible entity schema"));
            }
            if table
                .keys
                .iter()
                .zip(&keys)
                .any(|(column, key)| &payload[column] != key)
            {
                return Err(invalid("Entity identity does not match payload"));
            }
        }
        _ => return Err(invalid("Invalid entity deletion/payload")),
    }
    if let Some(payload)=record.payload.as_ref().and_then(Value::as_object) {
        if record.entity=="spaces" {
            let id=payload.get("id").and_then(Value::as_str).ok_or_else(||invalid("Invalid space identity"))?;
            if !uuid::Uuid::parse_str(id).is_ok_and(|value|value.to_string()==id){return Err(invalid("Invalid space identity"));}
        }
        for value in payload.values(){sql_value(value)?;}
        if record.entity=="attachments" {
            let hash=payload.get("hash").and_then(Value::as_str).ok_or_else(||invalid("Missing attachment hash"))?;
            if hash.len()!=64 || !hash.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || payload.get("size").is_some_and(|v|!v.is_null() && v.as_i64().is_none_or(|n|n<0 || n>512*1024*1024)){return Err(invalid("Invalid attachment metadata"));}
        }
        for name in ["minutes_before","reminder_minutes"] {
            if let Some(value)=payload.get(name).filter(|v|!v.is_null()){if value.as_i64().is_none_or(|n| !(0..=525600).contains(&n)){return Err(invalid("Reminder interval exceeds supported limits"));}}
        }
        for name in ["start_at","end_at","reminder_at","original_start_at","new_start_at"] {
            if let Some(value)=payload.get(name).filter(|v|!v.is_null()) {
                if value.as_str().is_none_or(|v|chrono::NaiveDateTime::parse_from_str(v,"%Y-%m-%dT%H:%M:%S").is_err()){return Err(invalid("Invalid exchanged timestamp"));}
            }
        }
        if record.entity=="sections" && payload.get("is_encrypted").and_then(Value::as_i64)==Some(1) {
            let parameters:crate::crypto::kdf::KdfParams=serde_json::from_str(payload.get("kdf_params").and_then(Value::as_str).ok_or_else(||invalid("Missing key parameters"))?).map_err(|_|invalid("Invalid key parameters"))?;
            if !(8..=262144).contains(&parameters.m_cost) || !(1..=10).contains(&parameters.t_cost) || !(1..=8).contains(&parameters.p_cost){return Err(invalid("Exchanged key parameters exceed supported limits"));}
            parameters.to_argon2()?;
        }
    }
    Ok(table)
}

pub fn variant_id(record: &Record) -> VaultResult<String> {
    Ok(crate::blob_store::hash_hex(encode(record)?.as_bytes()))
}

pub fn retain_conflict(conn: &Connection, record: &Record) -> VaultResult<()> {
    conn.execute("INSERT OR IGNORE INTO sync_conflicts(entity,entity_key,variant_id,context,payload,deleted) VALUES(?1,?2,?3,?4,?5,?6)",
        params![record.entity,record.key,variant_id(record)?,encode(&record.context)?,record.payload.as_ref().map(encode).transpose()?,record.deleted])?;
    Ok(())
}

pub fn conflicts(conn: &Connection) -> VaultResult<Vec<Record>> {
    let mut stmt = conn.prepare("SELECT entity,entity_key,context,payload,deleted FROM sync_conflicts ORDER BY entity,entity_key,variant_id")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(entity, key, context, payload, deleted)| {
            Ok(Record {
                entity,
                key,
                context: decode(&context)?,
                payload: payload.as_deref().map(decode).transpose()?,
                deleted,
            })
        })
        .collect()
}

pub fn get(conn: &Connection, entity: &str, key: &str) -> VaultResult<Option<Record>> {
    let row = conn
        .query_row(
            "SELECT context,payload,deleted FROM sync_objects WHERE entity=?1 AND entity_key=?2",
            params![entity, key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            },
        )
        .optional()?;
    row.map(|(context, payload, deleted)| {
        Ok(Record {
            entity: entity.into(),
            key: key.into(),
            context: decode(&context)?,
            payload: payload.as_deref().map(decode).transpose()?,
            deleted,
        })
    })
    .transpose()
}

pub fn save_head(conn: &Connection, record: &Record) -> VaultResult<()> {
    conn.execute("INSERT INTO sync_objects(entity,entity_key,context,payload,deleted) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(entity,entity_key) DO UPDATE SET context=excluded.context,payload=excluded.payload,deleted=excluded.deleted",params![record.entity,record.key,encode(&record.context)?,record.payload.as_ref().map(encode).transpose()?,record.deleted])?;
    Ok(())
}

fn sql_value(value: &Value) -> VaultResult<rusqlite::types::Value> {
    use rusqlite::types::Value as Sql;
    Ok(match value {
        Value::Null => Sql::Null,
        Value::Bool(value) => Sql::Integer(i64::from(*value)),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Sql::Integer(integer)
            } else {
                return Err(invalid("Unsupported numeric value"));
            }
        }
        Value::String(value) => Sql::Text(value.clone()),
        Value::Object(value) if value.len() == 1 && value.contains_key("$blob") => {
            let hex = value["$blob"]
                .as_str()
                .ok_or_else(|| invalid("Invalid binary payload"))?;
            if hex.len() % 2 != 0
                || hex.len() > 32 * 1024 * 1024
                || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(invalid("Invalid binary encoding"));
            }
            Sql::Blob(
                hex.as_bytes()
                    .chunks(2)
                    .map(|part| u8::from_str_radix(std::str::from_utf8(part).unwrap(), 16).unwrap())
                    .collect(),
            )
        }
        _ => return Err(invalid("Unsupported field value")),
    })
}

pub fn materialize(conn: &Connection, record: &Record) -> VaultResult<()> {
    let table = validate(conn, record)?;
    let keys: Vec<Value> = decode(&record.key)?;
    let conditions = table
        .keys
        .iter()
        .map(|column| format!("\"{column}\"=?"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let key_values = keys
        .iter()
        .map(sql_value)
        .collect::<VaultResult<Vec<_>>>()?;
    if record.deleted {
        conn.execute(
            &format!("DELETE FROM \"{}\" WHERE {conditions}", table.name),
            rusqlite::params_from_iter(key_values),
        )?;
        return Ok(());
    }
    let mut table = table;
    let mut payload = record
        .payload
        .as_ref()
        .unwrap()
        .as_object()
        .unwrap()
        .clone();
    if table.name == "spaces" {
        let id = payload
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("Invalid space identity"))?;
        if !uuid::Uuid::parse_str(id).is_ok_and(|value|value.to_string()==id){return Err(invalid("Invalid space identity"));}
        // Device-local registry paths are constructed here, never accepted from a peer.
        payload.insert("db_file".into(), Value::String(format!("spaces/{id}.db")));
        table.columns.push("db_file".into());
    }
    let values = table
        .columns
        .iter()
        .map(|column| sql_value(&payload[column]))
        .collect::<VaultResult<Vec<_>>>()?;
    let set = table
        .columns
        .iter()
        .map(|column| format!("\"{column}\"=?"))
        .collect::<Vec<_>>()
        .join(",");
    let changed = conn.execute(
        &format!("UPDATE \"{}\" SET {set} WHERE {conditions}", table.name),
        rusqlite::params_from_iter(values.iter().chain(&key_values)),
    )?;
    if changed == 0 {
        let columns = table
            .columns
            .iter()
            .map(|column| format!("\"{column}\""))
            .collect::<Vec<_>>()
            .join(",");
        let placeholders = vec!["?"; values.len()].join(",");
        conn.execute(
            &format!(
                "INSERT INTO \"{}\" ({columns}) VALUES({placeholders})",
                table.name
            ),
            rusqlite::params_from_iter(values),
        )?;
    }
    Ok(())
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct ApplyResult {
    pub applied: usize,
    pub conflicts: usize,
    pub unchanged: usize,
}

/// Incoming records and all their changes are one owning-store transaction.
/// Local triggers are suppressed only within that transaction, never across
/// user operations. Constraint failure rolls back both data and sync metadata.
pub fn apply(conn: &mut Connection, incoming: &[Record]) -> VaultResult<ApplyResult> {
    for record in incoming {
        validate(conn, record)?;
    }
    let tx = conn.transaction()?;
    tx.execute_batch("PRAGMA defer_foreign_keys=ON; UPDATE sync_state SET importing=1;")?;
    let result = apply_records(&tx,incoming)?;
    tx.execute("UPDATE sync_state SET importing=0", [])?;
    tx.commit()?;
    Ok(result)
}

/// Caller owns the transaction and suppression flag, allowing domain metadata
/// and its records to commit together.
pub fn apply_records(tx: &Connection, incoming: &[Record]) -> VaultResult<ApplyResult> {
    for record in incoming { validate(tx,record)?; }
    let mut result = ApplyResult::default();
    let mut ordered=incoming.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|r|match r.entity.as_str(){"notebooks"|"task_lists"=>0,"sections"=>1,"pages"|"tasks"|"events"=>2,_=>3});
    for record in ordered {
        let current = get(&tx, &record.entity, &record.key)?;
        let mut heads = conflicts(&tx)?
            .into_iter()
            .filter(|head| head.entity == record.entity && head.key == record.key)
            .collect::<Vec<_>>();
        if let Some(current) = current {
            if !heads.contains(&current) {
                heads.push(current);
            }
        }
        if heads.iter().any(|head| {
            head.context == record.context
                && (head.payload != record.payload || head.deleted != record.deleted)
        }) {
            return Err(invalid("Peer reused a revision for different content"));
        }
        if heads
            .iter()
            .any(|head| dominates(&head.context, &record.context))
        {
            result.unchanged += 1;
            continue;
        }
        if !heads.is_empty() && heads.iter().all(|head|head.payload==record.payload && head.deleted==record.deleted) {
            let mut merged=record.clone();for head in &heads {merged.context=union(&merged.context,&head.context);}
            save_head(tx,&merged)?;
            tx.execute("DELETE FROM sync_conflicts WHERE entity=?1 AND entity_key=?2",params![record.entity,record.key])?;
            result.unchanged+=1;continue;
        }
        heads.retain(|head| !dominates(&record.context, &head.context));
        heads.push(record.clone());
        tx.execute(
            "DELETE FROM sync_conflicts WHERE entity=?1 AND entity_key=?2",
            params![record.entity, record.key],
        )?;
        if heads.len() == 1 {
            materialize(&tx, record)?;
            save_head(&tx, record)?;
            if record.entity=="tasks" && !record.deleted && exists(tx,"note_sync_queue")? {
                tx.execute("INSERT INTO note_sync_queue(id,task_id,source_page_ref,source_node_id,checked,source_context) SELECT lower(hex(randomblob(16))),id,source_page_ref,source_node_id,status='done',?1 FROM tasks WHERE id=json_extract(?2,'$[0]') AND source_page_ref IS NOT NULL AND source_node_id IS NOT NULL ON CONFLICT(task_id,source_page_ref,source_node_id) DO UPDATE SET checked=excluded.checked,source_context=excluded.source_context,status='pending',last_error=NULL",params![encode(&record.context)?,record.key])?;
            }
            result.applied += 1;
        } else {
            // Preserve the materialized local head until explicit resolution;
            // every alternative is durable and forwards to further peers.
            for head in &heads {
                retain_conflict(&tx, head)?;
            }
            result.conflicts += 1;
        }
    }
    validate_hierarchy(&tx)?;
    Ok(result)
}

pub(crate) fn validate_hierarchy(conn: &Connection) -> VaultResult<()> {
    for (table, parent) in [
        ("pages", "parent_page_id"),
        ("tasks", "parent_task_id"),
        ("section_groups", "parent_group_id"),
    ] {
        if !exists(conn, table)? {
            continue;
        }
        let mut stmt = conn.prepare(&format!("SELECT id,\"{parent}\" FROM \"{table}\""))?;
        let parents = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        for id in parents.keys() {
            let mut seen = std::collections::BTreeSet::new();
            let mut current = Some(id.clone());
            while let Some(id) = current {
                if !seen.insert(id.clone()) {
                    return Err(invalid("Synchronized hierarchy contains a cycle"));
                }
                current = parents.get(&id).cloned().flatten();
            }
        }
    }
    if exists(conn, "pages")? {
        let crosses_boundary: bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pages c JOIN pages p ON c.parent_page_id=p.id JOIN sections s ON p.section_id=s.id WHERE s.is_encrypted=1 AND c.section_id!=p.section_id)",[],|row| row.get(0))?;
        if crosses_boundary {
            return Err(invalid(
                "Synchronized hierarchy crosses a protected boundary",
            ));
        }
    }
    Ok(())
}

pub fn resolve(conn: &mut Connection, entity: &str, key: &str, variant: &str) -> VaultResult<()> {
    let tx = conn.savepoint()?;
    let heads = conflicts(&tx)?
        .into_iter()
        .filter(|record| record.entity == entity && record.key == key)
        .collect::<Vec<_>>();
    let mut chosen = heads
        .iter()
        .find(|head| variant_id(head).ok().as_deref() == Some(variant))
        .cloned()
        .ok_or_else(|| invalid("Conflict variant is no longer available"))?;
    chosen.context = heads.iter().fold(Context::new(), |context, head| {
        union(&context, &head.context)
    });
    let (origin, sequence): (String, u64) = tx.query_row(
        "UPDATE sync_state SET sequence=sequence+1,importing=1 RETURNING origin,sequence",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    chosen.context.insert(origin.clone(), sequence);
    tx.execute(
        "INSERT INTO sync_changes(origin,sequence,entity,entity_key) VALUES(?1,?2,?3,?4)",
        params![origin, sequence, entity, key],
    )?;
    materialize(&tx, &chosen)?;
    validate_hierarchy(&tx)?;
    save_head(&tx, &chosen)?;
    tx.execute(
        "DELETE FROM sync_conflicts WHERE entity=?1 AND entity_key=?2",
        params![entity, key],
    )?;
    tx.execute("UPDATE sync_state SET importing=0", [])?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE task_lists(id TEXT PRIMARY KEY,name TEXT,color TEXT,sort_order INTEGER);CREATE TABLE tasks(id TEXT PRIMARY KEY,title TEXT);").unwrap();
        conn.execute_batch(include_str!("../../migrations/common/replication.sql"))
            .unwrap();
        install(&mut conn).unwrap();
        conn
    }
    #[test]
    fn revision_and_mutation_roll_back_together() {
        let mut conn = database();
        {
            let tx = conn.transaction().unwrap();
            tx.execute("INSERT INTO tasks(id,title) VALUES('one','draft')", [])
                .unwrap();
        }
        assert!(inventory(&conn).unwrap().is_empty());
        conn.execute("INSERT INTO tasks(id,title) VALUES('one','committed')", [])
            .unwrap();
        let first = inventory(&conn).unwrap().remove(0);
        conn.execute("DELETE FROM tasks WHERE id='one'", [])
            .unwrap();
        let deleted = inventory(&conn).unwrap().remove(0);
        assert!(deleted.deleted);
        assert!(deleted.payload.is_none());
        assert!(dominates(&deleted.context, &first.context));
    }
    #[test]
    fn protected_update_does_not_retain_old_plaintext_payload() {
        let conn = database();
        conn.execute(
            "INSERT INTO tasks(id,title) VALUES('one','secret before protection')",
            [],
        )
        .unwrap();
        conn.execute("UPDATE tasks SET title='ciphertext' WHERE id='one'", [])
            .unwrap();
        assert!(!encode(&inventory(&conn).unwrap())
            .unwrap()
            .contains("secret before protection"));
        let changes: String = conn
            .query_row(
                "SELECT group_concat(entity_key) FROM sync_changes",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!changes.contains("secret"));
    }
    #[test]
    fn contexts_detect_concurrency_and_clock_independent_successors() {
        let a = BTreeMap::from([("a".into(), 2)]);
        let b = BTreeMap::from([("a".into(), 1), ("b".into(), 1)]);
        assert!(!dominates(&a, &b));
        assert!(!dominates(&b, &a));
        let merged = union(&a, &b);
        assert!(dominates(&merged, &a));
        assert!(dominates(&merged, &b));
    }
}
