//! 页面读写与版本快照（spec: 富文本编辑与内容存储 / 页面版本历史；design D3/D6）。
//!
//! - 内容透明加解密：加密分区经 `SessionManager` 取 DSK 后 seal/open，
//!   存储表示为 base64(v1 tag || ciphertext || nonce)；锁定分区读写一律
//!   返回 `SectionLocked`（spec: 解锁会话状态一致性）。
//! - 快照节流（design D6）：距上一快照超过阈值（默认 5 分钟）或无快照时，
//!   保存前为旧内容插入一条 `page_versions`；回滚产生新快照而非删除历史。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::session::SessionManager;
use super::{new_id, protect_content, reveal_content, section_encrypted};
use crate::error::{VaultError, VaultResult};

/// 快照节流阈值（design D6）。
const SNAPSHOT_INTERVAL_MINUTES: i64 = 5;

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub id: String,
    pub section_id: String,
    pub parent_page_id: Option<String>,
    pub title: String,
    /// 明文内容（TipTap JSON 或纯文本）
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageVersion {
    pub id: String,
    pub page_id: String,
    pub created_at: String,
    /// 明文内容（加密分区的密文快照在返回前解密；锁定时不可读）
    pub content: String,
}

struct PageRow {
    id: String,
    section_id: String,
    parent_page_id: Option<String>,
    title: String,
    content: String,
    created_at: String,
    updated_at: String,
    sort_order: i64,
    is_deleted: bool,
}

fn row_page(r: &rusqlite::Row<'_>) -> rusqlite::Result<PageRow> {
    Ok(PageRow {
        id: r.get(0)?,
        section_id: r.get(1)?,
        parent_page_id: r.get(2)?,
        title: r.get(3)?,
        content: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        sort_order: r.get(7)?,
        is_deleted: r.get::<_, i64>(8)? != 0,
    })
}

const PAGE_COLS: &str =
    "id, section_id, parent_page_id, title, content, created_at, updated_at, sort_order, is_deleted";

fn load_page(conn: &Connection, id: &str) -> VaultResult<PageRow> {
    let sql = format!("SELECT {PAGE_COLS} FROM pages WHERE id = ?1");
    conn.query_row(&sql, params![id], row_page)
        .optional()?
        .ok_or_else(|| VaultError::NotFound(format!("页面 {id}")))
}

/// 读取页面（明文内容）。加密分区锁定时返回 `SectionLocked`。
pub fn get_page(conn: &Connection, session: &SessionManager, id: &str) -> VaultResult<Page> {
    let row = load_page(conn, id)?;
    if row.is_deleted {
        return Err(VaultError::NotFound(format!("页面 {id} 已在回收站")));
    }
    let encrypted = section_encrypted(conn, &row.section_id)?;
    let content = reveal_content(session, &row.section_id, encrypted, &row.content)?;
    Ok(Page {
        id: row.id,
        section_id: row.section_id,
        parent_page_id: row.parent_page_id,
        title: row.title,
        content,
        created_at: row.created_at,
        updated_at: row.updated_at,
        sort_order: row.sort_order,
    })
}

/// 距上一快照是否超过节流阈值（或无快照）。
fn snapshot_due(conn: &Connection, page_id: &str) -> VaultResult<bool> {
    let recent: i64 = conn.query_row(
        "SELECT COUNT(*) FROM page_versions WHERE page_id = ?1 AND created_at > datetime('now', ?2)",
        params![page_id, format!("-{SNAPSHOT_INTERVAL_MINUTES} minutes")],
        |r| r.get(0),
    )?;
    Ok(recent == 0)
}

/// 插入一条快照（存储表示原样入快照：加密分区即密文快照，spec 要求密文存储）。
fn insert_snapshot(conn: &Connection, page_id: &str, stored_content: &str) -> VaultResult<()> {
    conn.execute(
        "INSERT INTO page_versions (id, page_id, content) VALUES (?1, ?2, ?3)",
        params![new_id(), page_id, stored_content],
    )?;
    Ok(())
}

/// 保存页面：透明加密写入 + 更新 `updated_at` + 节流快照旧内容。
pub fn save_page(
    conn: &mut Connection,
    session: &SessionManager,
    id: &str,
    title: &str,
    content: &str,
) -> VaultResult<()> {
    let row = load_page(conn, id)?;
    if row.is_deleted {
        return Err(VaultError::NotFound(format!("页面 {id} 已在回收站")));
    }
    let encrypted = section_encrypted(conn, &row.section_id)?;
    let tx = conn.transaction()?;
    if snapshot_due(&tx, id)? {
        insert_snapshot(&tx, id, &row.content)?;
    }
    let stored = protect_content(session, &row.section_id, encrypted, content)?;
    tx.execute(
        "UPDATE pages SET title = ?1, content = ?2, updated_at = datetime('now') WHERE id = ?3",
        params![title, stored, id],
    )?;
    tx.commit()?;
    Ok(())
}

/// 版本历史（按时间倒序，返回明文）。加密分区锁定时返回 `SectionLocked`。
pub fn list_versions(
    conn: &Connection,
    session: &SessionManager,
    page_id: &str,
) -> VaultResult<Vec<PageVersion>> {
    let row = load_page(conn, page_id)?;
    let encrypted = section_encrypted(conn, &row.section_id)?;
    // 锁定分区版本历史不可读（spec: 锁定后版本历史不可读）
    if encrypted {
        session.with_dsk(&row.section_id, |_| Ok(()))?;
    }
    let mut stmt = conn.prepare(
        "SELECT id, page_id, created_at, content FROM page_versions WHERE page_id = ?1 ORDER BY created_at DESC",
    )?;
    let versions = stmt
        .query_map(params![page_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = Vec::with_capacity(versions.len());
    for (vid, created_at, stored) in versions {
        let content = reveal_content(session, &row.section_id, encrypted, &stored)?;
        out.push(PageVersion {
            id: vid,
            page_id: page_id.to_string(),
            created_at,
            content,
        });
    }
    Ok(out)
}

/// 回滚：当前内容变为目标版本内容；回滚前的当前内容生成新快照
/// （spec: 保留原当前版本与被选版本两条记录）。存储表示直接搬运，
/// 同一 DSK 下密文仍有效，无需重加密。
pub fn rollback_version(
    conn: &mut Connection,
    session: &SessionManager,
    page_id: &str,
    version_id: &str,
) -> VaultResult<()> {
    let row = load_page(conn, page_id)?;
    if row.is_deleted {
        return Err(VaultError::NotFound(format!("页面 {page_id} 已在回收站")));
    }
    let encrypted = section_encrypted(conn, &row.section_id)?;
    if encrypted {
        session.with_dsk(&row.section_id, |_| Ok(()))?;
    }
    let target: Option<String> = conn
        .query_row(
            "SELECT content FROM page_versions WHERE id = ?1 AND page_id = ?2",
            params![version_id, page_id],
            |r| r.get(0),
        )
        .optional()?;
    let target = target.ok_or_else(|| VaultError::NotFound(format!("版本 {version_id}")))?;
    let tx = conn.transaction()?;
    insert_snapshot(&tx, page_id, &row.content)?;
    tx.execute(
        "UPDATE pages SET content = ?1, updated_at = datetime('now') WHERE id = ?2",
        params![target, page_id],
    )?;
    tx.commit()?;
    Ok(())
}

/// 最近编辑时间（供前端快照节流判断的辅助）。
pub fn latest_version_at(conn: &Connection, page_id: &str) -> VaultResult<Option<String>> {
    let at = conn
        .query_row(
            "SELECT created_at FROM page_versions WHERE page_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![page_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(at)
}

#[cfg(test)]
mod tests {
    use super::super::sections_crypto;
    use super::*;
    use crate::crypto::kdf::KdfParams;
    use crate::db::migrate::{run_migrations, DbKind};
    use crate::notes::hierarchy;

    fn fast_params() -> KdfParams {
        KdfParams {
            m_cost: 8 * 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    struct Fixture {
        conn: Connection,
        session: SessionManager,
        section_id: String,
        page_id: String,
    }

    fn fixture(encrypted: bool) -> Fixture {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        let nb = hierarchy::create_notebook(&conn, "nb", None).unwrap();
        let section = hierarchy::create_section(&conn, &nb.id, None, "s", None).unwrap();
        let page = hierarchy::create_page(&conn, &section.id, None, "p").unwrap();
        let session = SessionManager::new();
        if encrypted {
            let keys = sections_crypto::set_password_with_params(
                &mut conn,
                &section.id,
                "pw-123",
                true,
                &fast_params(),
            )
            .unwrap();
            session.insert(&section.id, keys); // 设置成功后分区处于解锁态
        }
        Fixture {
            conn,
            session,
            section_id: section.id,
            page_id: page.id,
        }
    }

    #[test]
    fn save_updates_content_and_timestamp() {
        let mut fx = fixture(false);
        save_page(
            &mut fx.conn,
            &fx.session,
            &fx.page_id,
            "标题",
            "{\"doc\":1}",
        )
        .unwrap();
        let page = get_page(&fx.conn, &fx.session, &fx.page_id).unwrap();
        assert_eq!(page.content, "{\"doc\":1}");
        assert_eq!(page.title, "标题");
        assert!(!page.updated_at.is_empty());
    }

    #[test]
    fn snapshot_on_first_save_and_rollback_keeps_history() {
        let mut fx = fixture(false);
        save_page(&mut fx.conn, &fx.session, &fx.page_id, "p", "v1").unwrap();
        // 5 分钟阈值内再保存不产生新快照
        save_page(&mut fx.conn, &fx.session, &fx.page_id, "p", "v2").unwrap();
        let versions = list_versions(&fx.conn, &fx.session, &fx.page_id).unwrap();
        assert_eq!(versions.len(), 1, "节流阈值内只保留首条快照");
        assert_eq!(versions[0].content, "", "首条快照捕获的是保存前的旧内容");

        // 回滚 v1（空内容）→ 当前内容变更 + 历史增加而非删除
        rollback_version(&mut fx.conn, &fx.session, &fx.page_id, &versions[0].id).unwrap();
        let page = get_page(&fx.conn, &fx.session, &fx.page_id).unwrap();
        assert_eq!(page.content, "");
        let versions = list_versions(&fx.conn, &fx.session, &fx.page_id).unwrap();
        assert_eq!(versions.len(), 2, "回滚产生新快照，历史不丢");
    }

    #[test]
    fn encrypted_page_roundtrip_and_locked_rejected() {
        let mut fx = fixture(true);
        save_page(&mut fx.conn, &fx.session, &fx.page_id, "p", "机密内容").unwrap();
        // 库中落盘的是密文
        let stored: String = fx
            .conn
            .query_row(
                "SELECT content FROM pages WHERE id = ?1",
                params![fx.page_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!stored.contains("机密内容"), "密文不得含明文");
        // 解锁态读回明文
        let page = get_page(&fx.conn, &fx.session, &fx.page_id).unwrap();
        assert_eq!(page.content, "机密内容");

        // 锁定后读写均拒绝（SectionLocked 而非乱码）
        fx.session.lock(&fx.section_id);
        assert!(matches!(
            get_page(&fx.conn, &fx.session, &fx.page_id),
            Err(VaultError::SectionLocked(_))
        ));
        assert!(matches!(
            save_page(&mut fx.conn, &fx.session, &fx.page_id, "p", "x"),
            Err(VaultError::SectionLocked(_))
        ));
        assert!(matches!(
            list_versions(&fx.conn, &fx.session, &fx.page_id),
            Err(VaultError::SectionLocked(_))
        ));
    }

    #[test]
    fn encrypted_versions_stored_ciphertext() {
        let mut fx = fixture(true);
        save_page(&mut fx.conn, &fx.session, &fx.page_id, "p", "第一版").unwrap();
        // 人为制造间隔让第二次保存产生快照
        fx.conn
            .execute(
                "UPDATE page_versions SET created_at = datetime('now', '-10 minutes') WHERE page_id = ?1",
                params![fx.page_id],
            )
            .unwrap();
        save_page(&mut fx.conn, &fx.session, &fx.page_id, "p", "第二版").unwrap();
        let stored: String = fx
            .conn
            .query_row(
                "SELECT content FROM page_versions WHERE content != '' ORDER BY created_at DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!stored.contains("第一版"), "版本快照必须以密文存储");
        // 解锁态列表返回明文
        let versions = list_versions(&fx.conn, &fx.session, &fx.page_id).unwrap();
        assert!(versions.iter().any(|v| v.content == "第一版"));
    }
}
