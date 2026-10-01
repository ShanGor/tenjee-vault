//! Global tag dictionary and local association helpers.
//!
//! Tags are owned by meta.db. Domain databases keep only the stable tag id so
//! their data remains independently portable; callers resolve a missing id as
//! an orphan rather than treating it as a corrupt record.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

fn normalize_name(name: &str) -> VaultResult<&str> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(VaultError::Validation(
            "标签名称必须为 1 到 120 个字符".into(),
        ));
    }
    Ok(name)
}

pub fn list(meta: &Connection) -> VaultResult<Vec<Tag>> {
    let mut statement =
        meta.prepare("SELECT id, name, color FROM tags ORDER BY name COLLATE NOCASE, id")?;
    let tags = statement
        .query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tags)
}

pub fn create(meta: &Connection, name: &str, color: Option<&str>) -> VaultResult<Tag> {
    let name = normalize_name(name)?;
    if meta.query_row(
        "SELECT EXISTS(SELECT 1 FROM tags WHERE name = ?1 COLLATE NOCASE)",
        [name],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(VaultError::Validation(format!("标签名称已存在：{name}")));
    }
    let tag = Tag {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_owned(),
        color: color
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned),
    };
    meta.execute(
        "INSERT INTO tags (id, name, color) VALUES (?1, ?2, ?3)",
        params![tag.id, tag.name, tag.color],
    )?;
    Ok(tag)
}

pub fn update(meta: &Connection, id: &str, name: &str, color: Option<&str>) -> VaultResult<Tag> {
    let name = normalize_name(name)?;
    if meta.query_row(
        "SELECT EXISTS(SELECT 1 FROM tags WHERE name = ?1 COLLATE NOCASE AND id != ?2)",
        params![name, id],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(VaultError::Validation(format!("标签名称已存在：{name}")));
    }
    let tag = Tag {
        id: id.to_owned(),
        name: name.to_owned(),
        color: color
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned),
    };
    if meta.execute(
        "UPDATE tags SET name = ?1, color = ?2 WHERE id = ?3",
        params![tag.name, tag.color, tag.id],
    )? == 0
    {
        return Err(VaultError::NotFound(format!("标签 {id}")));
    }
    Ok(tag)
}

pub fn exists(meta: &Connection, id: &str) -> VaultResult<bool> {
    Ok(meta
        .query_row("SELECT 1 FROM tags WHERE id = ?1", params![id], |_| Ok(()))
        .optional()?
        .is_some())
}

pub fn delete(meta: &Connection, id: &str) -> VaultResult<()> {
    if meta.execute("DELETE FROM tags WHERE id = ?1", params![id])? == 0 {
        return Err(VaultError::NotFound(format!("标签 {id}")));
    }
    Ok(())
}

pub fn entity_tags(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> VaultResult<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT tag_id FROM taggings WHERE entity_type = ?1 AND entity_id = ?2 ORDER BY tag_id",
    )?;
    let tags = statement
        .query_map(params![entity_type, entity_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tags)
}

pub fn set_entity_tags(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    tag_ids: &[String],
) -> VaultResult<()> {
    let transaction = conn.unchecked_transaction()?;
    transaction.execute(
        "DELETE FROM taggings WHERE entity_type = ?1 AND entity_id = ?2",
        params![entity_type, entity_id],
    )?;
    let mut unique = std::collections::BTreeSet::new();
    for tag_id in tag_ids {
        if !unique.insert(tag_id) {
            continue;
        }
        transaction.execute(
            "INSERT OR IGNORE INTO taggings (tag_id, entity_type, entity_id) VALUES (?1, ?2, ?3)",
            params![tag_id, entity_type, entity_id],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn clear_tag(conn: &Connection, tag_id: &str) -> VaultResult<()> {
    conn.execute("DELETE FROM taggings WHERE tag_id = ?1", params![tag_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate::{run_migrations, DbKind};

    #[test]
    fn dictionary_crud_is_case_insensitive_and_associations_can_be_cleared() {
        let mut meta = Connection::open_in_memory().unwrap();
        let mut space = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&meta).unwrap();
        crate::db::connection::configure(&space).unwrap();
        run_migrations(&mut meta, DbKind::Meta.migrations()).unwrap();
        run_migrations(&mut space, DbKind::Space.migrations()).unwrap();

        let tag = create(&meta, " Project ", Some("#3366ff")).unwrap();
        assert_eq!(tag.name, "Project");
        assert!(create(&meta, "project", None).is_err());
        let tag = update(&meta, &tag.id, "Work", None).unwrap();
        assert_eq!(list(&meta).unwrap(), vec![tag.clone()]);

        set_entity_tags(&space, "page", "page-1", &[tag.id.clone(), tag.id.clone()]).unwrap();
        assert_eq!(
            entity_tags(&space, "page", "page-1").unwrap(),
            vec![tag.id.clone()]
        );
        clear_tag(&space, &tag.id).unwrap();
        assert!(entity_tags(&space, "page", "page-1").unwrap().is_empty());
        delete(&meta, &tag.id).unwrap();
        assert!(list(&meta).unwrap().is_empty());
    }
}
