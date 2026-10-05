//! Space-wide editable page tree. Legacy sections are private encryption domains.
use super::{hierarchy::PageSummary, session::SessionManager, *};
use crate::{
    crypto::keys::wrap_dsk,
    error::{VaultError, VaultResult},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub const PLAIN: &str = "__plain_pages__";

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    #[serde(flatten)]
    pub page: PageSummary,
    pub is_encrypted: bool,
    pub protection_root_id: Option<String>,
    pub children: Vec<Node>,
}

fn invalid(message: &str) -> VaultError {
    VaultError::Validation(message.into())
}

pub fn metadata(conn: &Connection, id: &str) -> VaultResult<Node> {
    conn.query_row(
        "SELECT p.id,p.section_id,p.parent_page_id,p.title,p.sort_order,p.updated_at,s.is_encrypted,s.root_page_id
         FROM pages p JOIN sections s ON s.id=p.section_id WHERE p.id=?1 AND p.is_deleted=0",
        [id], read_node,
    ).optional()?.ok_or_else(|| VaultError::NotFound("Page".into()))
}

fn read_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    Ok(Node {
        page: PageSummary {
            id: row.get(0)?,
            section_id: row.get(1)?,
            parent_page_id: row.get(2)?,
            title: row.get(3)?,
            sort_order: row.get(4)?,
            updated_at: row.get(5)?,
        },
        is_encrypted: row.get(6)?,
        protection_root_id: row.get(7)?,
        children: vec![],
    })
}

pub fn tree(conn: &Connection) -> VaultResult<Vec<Node>> {
    let mut statement = conn.prepare(
        "SELECT p.id,p.section_id,p.parent_page_id,p.title,p.sort_order,p.updated_at,s.is_encrypted,s.root_page_id
         FROM pages p JOIN sections s ON s.id=p.section_id WHERE p.is_deleted=0 ORDER BY p.sort_order,p.created_at,p.id"
    )?;
    let nodes = statement
        .query_map([], read_node)?
        .collect::<Result<Vec<_>, _>>()?;
    let ids: HashSet<_> = nodes.iter().map(|n| n.page.id.as_str()).collect();
    let mut children: HashMap<Option<String>, Vec<Node>> = HashMap::new();
    for node in &nodes {
        let parent = node
            .page
            .parent_page_id
            .clone()
            .filter(|id| ids.contains(id.as_str()));
        children.entry(parent).or_default().push(node.clone());
    }
    fn build(
        parent: Option<String>,
        children: &mut HashMap<Option<String>, Vec<Node>>,
    ) -> Vec<Node> {
        let mut out = children.remove(&parent).unwrap_or_default();
        for node in &mut out {
            node.children = build(Some(node.page.id.clone()), children);
        }
        out
    }
    Ok(build(None, &mut children))
}

pub fn descendants(conn: &Connection, id: &str) -> VaultResult<Vec<String>> {
    let mut statement = conn.prepare(
        "WITH RECURSIVE subtree(id) AS (SELECT id FROM pages WHERE id=?1 UNION SELECT p.id FROM pages p JOIN subtree t ON p.parent_page_id=t.id) SELECT id FROM subtree"
    )?;
    let rows = statement
        .query_map([id], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn target_domain(
    conn: &Connection,
    session: &SessionManager,
    parent: Option<&str>,
) -> VaultResult<String> {
    if let Some(parent) = parent {
        let node = metadata(conn, parent)?;
        if node.is_encrypted {
            session.with_dsk(&node.page.section_id, |_| Ok(()))?;
            return Ok(node.page.section_id);
        }
    }
    Ok(PLAIN.into())
}

pub fn create(
    conn: &Connection,
    session: &SessionManager,
    parent: Option<&str>,
    title: &str,
) -> VaultResult<PageSummary> {
    let domain = target_domain(conn, session, parent)?;
    let encrypted=section_encrypted(conn, &domain)?;
    let stored_title=protect_content(session,&domain,encrypted,title)?;
    let mut page = hierarchy::create_page_prepared(conn, &domain, parent, &stored_title,encrypted)?;
    page.title=title.into();
    let content = protect_content(session, &domain, section_encrypted(conn, &domain)?, "")?;
    conn.execute("UPDATE pages SET content=?1,sort_order=(SELECT COALESCE(MAX(sort_order),-1)+1 FROM pages WHERE parent_page_id IS ?2 AND id!=?3) WHERE id=?3",params![content,parent,page.id])?;
    Ok(page)
}

/// Rewrite content, histories and blobs before changing a page's crypto domain.
/// Blob hashes are content addressed; old blobs are removed only after commit.
fn transfer(
    conn: &Connection,
    files: &Path,
    session: &SessionManager,
    ids: &[String],
    target: &str,
    old_blobs: &mut Vec<String>,
    new_blobs: &mut Vec<String>,
) -> VaultResult<()> {
    let target_encrypted = section_encrypted(conn, target)?;
    for id in ids {
        let (source, content, title, title_flag): (String, String, String, bool) = conn.query_row(
            "SELECT section_id,content,title,title_is_encrypted FROM pages WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        if source == target {
            continue;
        }
        let source_encrypted = section_encrypted(conn, &source)?;
        let plaintext = reveal_content(session, &source, source_encrypted, &content)?;
        let stored = protect_content(session, target, target_encrypted, &plaintext)?;
        let versions = {
            let mut stmt = conn.prepare("SELECT id,content FROM page_versions WHERE page_id=?1")?;
            let rows = stmt
                .query_map([id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (version, content) in versions {
            let plaintext = reveal_content(session, &source, source_encrypted, &content)?;
            conn.execute(
                "UPDATE page_versions SET content=?1 WHERE id=?2",
                params![
                    protect_content(session, target, target_encrypted, &plaintext)?,
                    version
                ],
            )?;
        }
        for attachment in attachments::list_attachments(conn, "page", id)? {
            let bytes = std::fs::read(crate::blob_store::blob_path(files, &attachment.hash))?;
            let plain = reveal_bytes(session, &source, source_encrypted, &bytes)?;
            let stored = protect_bytes(session, target, target_encrypted, &plain)?;
            let hash = crate::blob_store::store(files, &stored)?;
            new_blobs.push(hash.clone());
            conn.execute(
                "UPDATE attachments SET hash=?1 WHERE id=?2",
                params![hash, attachment.id],
            )?;
            old_blobs.push(attachment.hash);
        }
        conn.execute(
            "UPDATE pages SET section_id=?1,content=?2,title=?4,title_is_encrypted=?5 WHERE id=?3",
            params![target, stored, id, protect_content(session,target,target_encrypted,&reveal_content(session,&source,source_encrypted && title_flag,&title)?)?, target_encrypted],
        )?;
    }
    Ok(())
}

fn cleanup(conn: &Connection, files: &Path, hashes: &[String]) -> VaultResult<()> {
    for hash in hashes {
        crate::blob_store::remove_if_unref(files, conn, hash)?;
    }
    Ok(())
}

fn with_blob_transaction(
    conn: &mut Connection,
    files: &Path,
    work: impl FnOnce(&Connection, &mut Vec<String>, &mut Vec<String>) -> VaultResult<()>,
) -> VaultResult<()> {
    let mut old_blobs = vec![];
    let mut new_blobs = vec![];
    let result = (|| {
        let tx = conn.transaction()?;
        work(&tx, &mut old_blobs, &mut new_blobs)?;
        tx.commit()?;
        Ok(())
    })();
    if let Err(error) = result {
        cleanup(conn, files, &new_blobs)?;
        return Err(error);
    }
    cleanup(conn, files, &old_blobs)
}

pub fn move_subtree(
    conn: &mut Connection,
    files: &Path,
    session: &SessionManager,
    id: &str,
    parent: Option<&str>,
    order: i64,
) -> VaultResult<()> {
    let node = metadata(conn, id)?;
    let ids = descendants(conn, id)?;
    if parent.is_some_and(|parent| ids.iter().any(|id| id == parent)) {
        return Err(invalid(
            "A page cannot be moved beneath itself or its children",
        ));
    }
    let target = target_domain(conn, session, parent)?;
    if node.is_encrypted {
        session.with_dsk(&node.page.section_id, |_| Ok(()))?;
        if node.protection_root_id.as_deref() != Some(id) && target != node.page.section_id {
            return Err(invalid("Remove protection from the parent page before moving this child outside its protected subtree"));
        }
        if target != PLAIN && target != node.page.section_id {
            return Err(invalid(
                "Independent protected pages cannot be nested; remove one protection first",
            ));
        }
    }
    let mut plain_ids = Vec::new();
    for page in &ids {
        let domain: String =
            conn.query_row("SELECT section_id FROM pages WHERE id=?1", [page], |r| {
                r.get(0)
            })?;
        if section_encrypted(conn, &domain)? {
            if target != PLAIN && target != domain {
                return Err(invalid(
                    "Independent protected pages cannot be nested; remove one protection first",
                ));
            }
        } else {
            plain_ids.push(page.clone());
        }
    }
    with_blob_transaction(conn, files, |tx, old_blobs, new_blobs| {
        transfer(
            tx, files, session, &plain_ids, &target, old_blobs, new_blobs,
        )?;
        let mut siblings = {
            let mut stmt = tx.prepare("SELECT id FROM pages WHERE parent_page_id IS ?1 AND id!=?2 AND is_deleted=0 ORDER BY sort_order,created_at,id")?;
            let rows = stmt
                .query_map(params![parent, id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        siblings.insert((order.max(0) as usize).min(siblings.len()), id.to_owned());
        tx.execute(
            "UPDATE pages SET parent_page_id=?1 WHERE id=?2",
            params![parent, id],
        )?;
        for (index, id) in siblings.iter().enumerate() {
            tx.execute(
                "UPDATE pages SET sort_order=?1 WHERE id=?2",
                params![index as i64, id],
            )?;
        }
        Ok(())
    })
}

pub fn recycle(conn: &mut Connection, session: &SessionManager, id: &str) -> VaultResult<()> {
    metadata(conn, id)?;
    let tx = conn.transaction()?;
    for page in descendants(&tx, id)? {
        let domain: String =
            tx.query_row("SELECT section_id FROM pages WHERE id=?1", [&page], |r| {
                r.get(0)
            })?;
        if section_encrypted(&tx, &domain)? {
            session.with_dsk(&domain, |_| Ok(()))?;
        }
        tx.execute("UPDATE pages SET is_deleted=1,deleted_batch=?1,updated_at=datetime('now') WHERE id=?2 AND is_deleted=0",params![id,page])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn restore(conn: &mut Connection, session: &SessionManager, id: &str) -> VaultResult<()> {
    let tx = conn.transaction()?;
    // Restoring a child also restores its ancestors, preserving inherited protection.
    let mut ancestor = Some(id.to_owned());
    let mut seen = HashSet::new();
    while let Some(current) = ancestor {
        if !seen.insert(current.clone()) {
            return Err(invalid("Invalid page hierarchy"));
        }
        ancestor = tx
            .query_row(
                "SELECT parent_page_id FROM pages WHERE id=?1",
                [&current],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .ok_or_else(|| VaultError::NotFound("Page".into()))?;
        let domain: String = tx.query_row(
            "SELECT section_id FROM pages WHERE id=?1",
            [&current],
            |r| r.get(0),
        )?;
        if section_encrypted(&tx, &domain)? {
            session.with_dsk(&domain, |_| Ok(()))?;
        }
        tx.execute(
            "UPDATE pages SET is_deleted=0,deleted_batch=NULL WHERE id=?1",
            [&current],
        )?;
    }
    let batch = {
        let mut stmt =
            tx.prepare("SELECT DISTINCT section_id FROM pages WHERE deleted_batch=?1")?;
        let rows = stmt
            .query_map([id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for domain in batch {
        if section_encrypted(&tx, &domain)? {
            session.with_dsk(&domain, |_| Ok(()))?;
        }
    }
    tx.execute(
        "UPDATE pages SET is_deleted=0,deleted_batch=NULL WHERE deleted_batch=?1",
        [id],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn purge(conn: &mut Connection, files: &Path, id: &str) -> VaultResult<()> {
    let deleted: bool = conn.query_row("SELECT is_deleted FROM pages WHERE id=?1", [id], |r| {
        r.get(0)
    })?;
    if !deleted {
        return Err(invalid("Only recycled pages can be permanently deleted"));
    }
    let tx = conn.transaction()?;
    let ids = descendants(&tx, id)?;
    let mut old_blobs = vec![];
    for page in &ids {
        tx.execute("UPDATE pages SET parent_page_id=NULL WHERE id=?1", [page])?;
    }
    for page in ids.iter().rev() {
        let deleted: bool =
            tx.query_row("SELECT is_deleted FROM pages WHERE id=?1", [page], |r| {
                r.get(0)
            })?;
        if !deleted {
            return Err(invalid(
                "Restore or recycle the remaining children before deleting this page",
            ));
        }
        old_blobs.extend(
            attachments::list_attachments(&tx, "page", page)?
                .into_iter()
                .map(|a| a.hash),
        );
        tx.execute(
            "DELETE FROM attachments WHERE entity_type='page' AND entity_id=?1",
            [page],
        )?;
        tx.execute("DELETE FROM page_versions WHERE page_id=?1", [page])?;
        tx.execute(
            "DELETE FROM taggings WHERE entity_type='page' AND entity_id=?1",
            [page],
        )?;
        tx.execute("DELETE FROM pages WHERE id=?1", [page])?;
    }
    tx.commit()?;
    cleanup(conn, files, &old_blobs)
}

pub fn protect(
    conn: &mut Connection,
    files: &Path,
    session: &SessionManager,
    id: &str,
    password: &str,
    confirmed: bool,
) -> VaultResult<String> {
    if !confirmed || password.is_empty() {
        return Err(invalid(
            "Confirm password loss is unrecoverable and enter a password",
        ));
    }
    metadata(conn, id)?;
    let ids = descendants(conn, id)?;
    for page in &ids {
        let domain: String =
            conn.query_row("SELECT section_id FROM pages WHERE id=?1", [page], |r| {
                r.get(0)
            })?;
        if section_encrypted(conn, &domain)? {
            return Err(invalid(
                "This page tree already contains protected content; remove that protection first",
            ));
        }
    }
    let domain = new_id();
    let (wrapped, _) = wrap_dsk(password)?;
    let keys = crate::crypto::keys::unwrap_dsk(&wrapped, password)?;
    session.insert(&domain, keys);
    let result = with_blob_transaction(conn, files, |tx, old_blobs, new_blobs| {
        tx.execute("INSERT INTO sections(id,notebook_id,name,is_encrypted,kdf_salt,kdf_params,verifier,wrapped_dsk,root_page_id) VALUES (?1,'__page_storage__','Protected page',1,?2,?3,?4,?5,?6)",params![domain,wrapped.salt,serde_json::json!({"m_cost":wrapped.m_cost,"t_cost":wrapped.t_cost,"p_cost":wrapped.p_cost}).to_string(),wrapped.verifier,wrapped.wrapped_dsk,id])?;
        transfer(tx, files, session, &ids, &domain, old_blobs, new_blobs)?;
        Ok(())
    });
    if let Err(error) = result {
        session.lock(&domain);
        return Err(error);
    }
    Ok(domain)
}

pub fn remove_protection(
    conn: &mut Connection,
    files: &Path,
    session: &SessionManager,
    domain: &str,
    password: &str,
) -> VaultResult<()> {
    let keys = sections_crypto::unlock_keys(conn, domain, password)?;
    let templates: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM section_templates WHERE section_id=?1)",
        [domain],
        |r| r.get(0),
    )?;
    if templates {
        return Err(invalid(
            "Delete or export this protected page's private templates before removing its password",
        ));
    }
    session.insert(domain, keys);
    let ids = {
        let mut stmt = conn.prepare("SELECT id FROM pages WHERE section_id=?1")?;
        let rows = stmt
            .query_map([domain], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        rows
    };
    with_blob_transaction(conn, files, |tx, old_blobs, new_blobs| {
        transfer(tx, files, session, &ids, PLAIN, old_blobs, new_blobs)?;
        tx.execute("UPDATE sections SET is_encrypted=0,kdf_salt=NULL,kdf_params=NULL,verifier=NULL,wrapped_dsk=NULL WHERE id=?1",[domain])?;
        Ok(())
    })?;
    session.lock(domain);
    crate::search::drop_unlocked_index(conn, domain)?;
    Ok(())
}

pub fn path(conn: &Connection, session: &SessionManager, id: &str) -> VaultResult<String> {
    let mut titles = vec![];
    let mut current = Some(id.to_owned());
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            break;
        }
        let row: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT title,parent_page_id FROM pages WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((title, parent)) = row else {
            break;
        };
        let _=title;
        titles.push(visible_title(conn,session,&id)?);
        current = parent;
    }
    titles.reverse();
    Ok(titles.join(" / "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate::{run_migrations, DbKind};

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        conn
    }

    #[test]
    fn migration_preserves_hierarchy_ids_and_encrypted_data() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, &DbKind::Space.migrations()[..3]).unwrap();
        let nb = hierarchy::create_notebook(&conn, "Work", None).unwrap();
        let group = hierarchy::create_section_group(&conn, &nb.id, None, "Projects").unwrap();
        let nested =
            hierarchy::create_section_group(&conn, &nb.id, Some(&group.id), "Alpha").unwrap();
        let section =
            hierarchy::create_section(&conn, &nb.id, Some(&nested.id), "Private", None).unwrap();
        let page = hierarchy::create_page(&conn, &section.id, None, "Notes").unwrap();
        let child = hierarchy::create_page(&conn, &section.id, Some(&page.id), "Child").unwrap();
        let session = SessionManager::new();
        pages::save_page(&mut conn, &session, &page.id, "Notes", "confidential").unwrap();
        let keys = sections_crypto::set_password(&mut conn, &section.id, "password", true).unwrap();
        session.insert(&section.id, keys);
        let ciphertext: String = conn
            .query_row("SELECT content FROM pages WHERE id=?1", [&page.id], |r| {
                r.get(0)
            })
            .unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        let roots = tree(&conn).unwrap();
        assert_eq!(roots.len(), 1);
        let parent = &roots[0].children[0].children[0].children[0];
        assert_eq!(parent.page.title, "Private");
        assert_eq!(parent.children[0].page.id, page.id);
        assert_eq!(parent.children[0].children[0].page.id, child.id);
        assert_eq!(
            parent.protection_root_id.as_deref(),
            Some(parent.page.id.as_str())
        );
        assert_eq!(
            pages::get_page(&conn, &session, &page.id).unwrap().content,
            "confidential"
        );
        assert_eq!(
            pages::get_page(&conn, &session, &parent.page.id)
                .unwrap()
                .content,
            ""
        );
        assert_eq!(
            conn.query_row("SELECT content FROM pages WHERE id=?1", [&page.id], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            ciphertext
        );
        assert!(crate::search::search(&conn, "confidential", &[], 100)
            .unwrap()
            .is_empty());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
    }

    #[test]
    fn subtree_moves_reject_cycles_and_recycle_restore_preserves_independent_deletions() {
        let mut conn = db();
        let session = SessionManager::new();
        let files = tempfile::tempdir().unwrap();
        assert!(tree(&conn).unwrap().is_empty());
        let a = create(&conn, &session, None, "A").unwrap();
        let b = create(&conn, &session, None, "B").unwrap();
        let child = create(&conn, &session, Some(&a.id), "Child").unwrap();
        let grandchild = create(&conn, &session, Some(&child.id), "Grandchild").unwrap();
        assert!(move_subtree(
            &mut conn,
            files.path(),
            &session,
            &a.id,
            Some(&grandchild.id),
            0
        )
        .is_err());
        move_subtree(&mut conn, files.path(), &session, &child.id, Some(&b.id), 0).unwrap();
        assert_eq!(
            path(&conn, &session, &grandchild.id).unwrap(),
            "B / Child / Grandchild"
        );
        recycle(&mut conn, &session, &grandchild.id).unwrap();
        recycle(&mut conn, &session, &b.id).unwrap();
        restore(&mut conn, &session, &b.id).unwrap();
        assert!(metadata(&conn, &child.id).is_ok());
        assert!(metadata(&conn, &grandchild.id).is_err());
        restore(&mut conn, &session, &grandchild.id).unwrap();
        assert!(metadata(&conn, &grandchild.id).is_ok());
        recycle(&mut conn, &session, &b.id).unwrap();
        purge(&mut conn, files.path(), &b.id).unwrap();
        assert_eq!(tree(&conn).unwrap().len(), 1);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
    }

    #[test]
    fn protection_covers_children_versions_attachments_and_moves() {
        let mut conn = db();
        let session = SessionManager::new();
        let files = tempfile::tempdir().unwrap();
        let root = create(&conn, &session, None, "Private").unwrap();
        let sibling = create(&conn, &session, None, "Public").unwrap();
        let child = create(&conn, &session, Some(&root.id), "Child").unwrap();
        pages::save_page(&mut conn, &session, &child.id, "Child", "old secret").unwrap();
        conn.execute("INSERT INTO page_versions(id,page_id,content) VALUES ('old-version',?1,'historic secret')",[&child.id]).unwrap();
        let attachment = attachments::save_attachment(
            files.path(),
            &conn,
            &session,
            PLAIN,
            "page",
            &child.id,
            "secret.txt",
            None,
            b"private bytes",
        )
        .unwrap();
        assert!(protect(
            &mut conn,
            files.path(),
            &session,
            &root.id,
            "password",
            false
        )
        .is_err());
        let domain = protect(
            &mut conn,
            files.path(),
            &session,
            &root.id,
            "password",
            true,
        )
        .unwrap();
        assert!(!metadata(&conn, &sibling.id).unwrap().is_encrypted);
        assert_eq!(
            pages::get_page(&conn, &session, &child.id).unwrap().content,
            "old secret"
        );
        assert!(pages::list_versions(&conn, &session, &child.id)
            .unwrap()
            .iter()
            .any(|v| v.content == "historic secret"));
        let data =
            attachments::open_attachment(files.path(), &conn, &session, &attachment.id).unwrap();
        assert_eq!(base64_decode(&data.data_base64).unwrap(), b"private bytes");
        assert!(!files.path().join(&attachment.hash).exists());
        assert!(
            !crate::blob_store::read(files.path(), &data.attachment.hash)
                .unwrap()
                .windows(13)
                .any(|w| w == b"private bytes")
        );
        assert!(crate::search::search(&conn, "secret", &[], 100)
            .unwrap()
            .is_empty());
        let inherited = create(&conn, &session, Some(&child.id), "New child").unwrap();
        assert_eq!(inherited.section_id, domain);
        assert!(pages::get_page(&conn, &session, &inherited.id).is_ok());
        assert!(move_subtree(
            &mut conn,
            files.path(),
            &session,
            &child.id,
            Some(&sibling.id),
            0
        )
        .is_err());
        let ordinary = create(&conn, &session, None, "Ordinary").unwrap();
        pages::save_page(&mut conn, &session, &ordinary.id, "Ordinary", "moved text").unwrap();
        move_subtree(
            &mut conn,
            files.path(),
            &session,
            &ordinary.id,
            Some(&root.id),
            0,
        )
        .unwrap();
        assert!(metadata(&conn, &ordinary.id).unwrap().is_encrypted);
        session.lock(&domain);
        assert!(matches!(
            pages::get_page(&conn, &session, &child.id),
            Err(VaultError::SectionLocked(_))
        ));
        assert!(pages::list_versions(&conn, &session, &child.id).is_err());
        assert!(
            attachments::open_attachment(files.path(), &conn, &session, &attachment.id).is_err()
        );
        assert!(create(&conn, &session, Some(&root.id), "Denied").is_err());
        assert!(remove_protection(&mut conn, files.path(), &session, &domain, "wrong").is_err());
        remove_protection(&mut conn, files.path(), &session, &domain, "password").unwrap();
        assert_eq!(
            pages::get_page(&conn, &session, &ordinary.id)
                .unwrap()
                .content,
            "moved text"
        );
        assert_eq!(
            pages::get_page(&conn, &session, &child.id).unwrap().content,
            "old secret"
        );
        assert_eq!(
            base64_decode(
                &attachments::open_attachment(files.path(), &conn, &session, &attachment.id)
                    .unwrap()
                    .data_base64
            )
            .unwrap(),
            b"private bytes"
        );
        assert!(pages::list_versions(&conn, &session, &child.id)
            .unwrap()
            .iter()
            .any(|v| v.content == "historic secret"));
        assert!(!metadata(&conn, &root.id).unwrap().is_encrypted);
        assert!(!crate::search::search(&conn, "secret", &[], 100)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn failed_decryption_rolls_back_and_removes_staged_plaintext_blobs() {
        let mut conn = db();
        let session = SessionManager::new();
        let files = tempfile::tempdir().unwrap();
        let root = create(&conn, &session, None, "Private").unwrap();
        let a = attachments::save_attachment(
            files.path(),
            &conn,
            &session,
            PLAIN,
            "page",
            &root.id,
            "a.txt",
            None,
            b"first private file",
        )
        .unwrap();
        let b = attachments::save_attachment(
            files.path(),
            &conn,
            &session,
            PLAIN,
            "page",
            &root.id,
            "b.txt",
            None,
            b"second private file",
        )
        .unwrap();
        let domain = protect(
            &mut conn,
            files.path(),
            &session,
            &root.id,
            "password",
            true,
        )
        .unwrap();
        let a_encrypted =
            attachments::open_attachment(files.path(), &conn, &session, &a.id).unwrap();
        let b_encrypted =
            attachments::open_attachment(files.path(), &conn, &session, &b.id).unwrap();
        std::fs::remove_file(crate::blob_store::blob_path(
            files.path(),
            &b_encrypted.attachment.hash,
        ))
        .unwrap();
        assert!(remove_protection(&mut conn, files.path(), &session, &domain, "password").is_err());
        assert!(metadata(&conn, &root.id).unwrap().is_encrypted);
        assert!(!crate::blob_store::blob_path(files.path(), &a.hash).exists());
        assert!(!crate::blob_store::blob_path(files.path(), &b.hash).exists());
        assert_eq!(
            attachments::open_attachment(files.path(), &conn, &session, &a.id)
                .unwrap()
                .attachment
                .hash,
            a_encrypted.attachment.hash
        );
    }

    #[test]
    fn exporting_page_subtree_includes_empty_parent_and_rejects_locked_children() {
        let mut conn = db();
        let session = SessionManager::new();
        let files = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let root = create(&conn, &session, None, "Overview").unwrap();
        let child = create(&conn, &session, Some(&root.id), "Private child").unwrap();
        let other = create(&conn, &session, None, "Unrelated").unwrap();
        pages::save_page(&mut conn,&session,&child.id,"Private child",r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"secret export"}]}]}"#).unwrap();
        let domain = protect(
            &mut conn,
            files.path(),
            &session,
            &child.id,
            "password",
            true,
        )
        .unwrap();
        move_subtree(
            &mut conn,
            files.path(),
            &session,
            &child.id,
            Some(&other.id),
            0,
        )
        .unwrap();
        assert_eq!(metadata(&conn, &child.id).unwrap().page.section_id, domain);
        move_subtree(
            &mut conn,
            files.path(),
            &session,
            &child.id,
            Some(&root.id),
            0,
        )
        .unwrap();
        session.lock(&domain);
        assert!(crate::portability::export::export_subtree(
            &conn,
            files.path(),
            &session,
            &root.id,
            crate::portability::export::NoteExportFormat::Markdown,
            output.path(),
            "locked",
            false
        )
        .is_err());
        assert!(!output.path().join("locked").exists());
        session.insert(
            &domain,
            sections_crypto::unlock_keys(&conn, &domain, "password").unwrap(),
        );
        crate::portability::export::export_subtree(
            &conn,
            files.path(),
            &session,
            &root.id,
            crate::portability::export::NoteExportFormat::Markdown,
            output.path(),
            "export",
            false,
        )
        .unwrap();
        assert!(output.path().join("export").is_dir());
        let entries = std::fs::read_dir(output.path().join("export"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(entries.len(), 2); // Overview document plus its children directory.
        assert!(!entries
            .iter()
            .any(|entry| entry.file_name().to_string_lossy().contains("Unrelated")));
    }
}

/// Hydrate navigation titles only for local unlocked views.
pub fn hydrate(conn:&Connection,session:&SessionManager,nodes:&mut [Node])->VaultResult<()> {
    for node in nodes {node.page.title=visible_title(conn,session,&node.page.id)?;hydrate(conn,session,&mut node.children)?;} Ok(())
}
pub fn hydrate_legacy(conn:&Connection,session:&SessionManager,nodes:&mut [hierarchy::PageNode])->VaultResult<()> {
    for node in nodes {node.page.title=visible_title(conn,session,&node.page.id)?;hydrate_legacy(conn,session,&mut node.children)?;} Ok(())
}
