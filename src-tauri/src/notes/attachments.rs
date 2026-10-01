//! 附件：内容寻址存储 + 引用计数（spec: 文件附件；design D7）。
//!
//! - 写入：SHA-256 内容哈希命名落盘 `<space_id>.files/<hash>`；同哈希只写一次。
//!   加密分区先 seal 再对密文取哈希/落盘（spec: 附件以密文写入附件目录）。
//! - 删除：数据库行删除后按哈希引用计数归零才物理移除文件。
//! - `size` 记录原始（明文）字节数，便于前端展示。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::session::SessionManager;
use super::{new_id, protect_bytes, reveal_bytes};
use crate::blob_store;
use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, Serialize)]
pub struct Attachment {
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub file_name: String,
    pub mime: Option<String>,
    pub size: i64,
    pub hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentData {
    #[serde(flatten)]
    pub attachment: Attachment,
    /// base64 编码的明文内容
    pub data_base64: String,
}

fn row_attachment(r: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: r.get(0)?,
        entity_type: r.get(1)?,
        entity_id: r.get(2)?,
        file_name: r.get(3)?,
        mime: r.get(4)?,
        size: r.get(5)?,
        hash: r.get(6)?,
        created_at: r.get(7)?,
    })
}

fn files_path(files_dir: &Path, hash: &str) -> PathBuf {
    blob_store::blob_path(files_dir, hash)
}

fn attachment_section_id(conn: &Connection, attachment: &Attachment) -> VaultResult<String> {
    if attachment.entity_type != "page" {
        return Err(VaultError::Validation(format!(
            "不支持的附件宿主类型 {}",
            attachment.entity_type
        )));
    }
    conn.query_row(
        "SELECT section_id FROM pages WHERE id = ?1",
        params![attachment.entity_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| VaultError::NotFound(format!("附件宿主页面 {}", attachment.entity_id)))
}

/// 保存附件：内容寻址落盘 + 数据库登记。返回登记记录。
pub fn save_attachment(
    files_dir: &Path,
    conn: &Connection,
    session: &SessionManager,
    section_id: &str,
    entity_type: &str,
    entity_id: &str,
    file_name: &str,
    mime: Option<&str>,
    bytes: &[u8],
) -> VaultResult<Attachment> {
    if bytes.is_empty() {
        return Err(VaultError::Validation("附件内容为空".into()));
    }
    let encrypted = super::section_encrypted(conn, section_id)?;
    let stored = protect_bytes(session, section_id, encrypted, bytes)?;
    let hash = blob_store::store(files_dir, &stored)?;
    let attachment = Attachment {
        id: new_id(),
        entity_type: entity_type.to_string(),
        entity_id: entity_id.to_string(),
        file_name: file_name.to_string(),
        mime: mime.map(str::to_string),
        size: bytes.len() as i64,
        hash,
        created_at: String::new(),
    };
    conn.execute(
        "INSERT INTO attachments (id, entity_type, entity_id, file_name, mime, size, hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            attachment.id,
            attachment.entity_type,
            attachment.entity_id,
            attachment.file_name,
            attachment.mime,
            attachment.size,
            attachment.hash
        ],
    )?;
    Ok(attachment)
}

fn load_attachment(conn: &Connection, attachment_id: &str) -> VaultResult<Attachment> {
    conn.query_row(
        "SELECT id, entity_type, entity_id, file_name, mime, size, hash, created_at FROM attachments WHERE id = ?1",
        params![attachment_id],
        row_attachment,
    )
    .optional()?
    .ok_or_else(|| VaultError::NotFound(format!("附件 {attachment_id}")))
}

/// 打开附件：读盘 + 按需解密，返回明文（加密分区锁定时 `SectionLocked`）。
pub fn open_attachment(
    files_dir: &Path,
    conn: &Connection,
    session: &SessionManager,
    attachment_id: &str,
) -> VaultResult<AttachmentData> {
    let attachment = load_attachment(conn, attachment_id)?;
    let section_id = attachment_section_id(conn, &attachment)?;
    let encrypted = super::section_encrypted(conn, &section_id)?;
    let stored = std::fs::read(files_path(files_dir, &attachment.hash))?;
    let plain = reveal_bytes(session, &section_id, encrypted, &stored)?;
    Ok(AttachmentData {
        attachment,
        data_base64: super::base64_encode(&plain),
    })
}

/// 删除附件：删登记行；同哈希引用计数归零后物理移除文件。
pub fn delete_attachment(
    files_dir: &Path,
    conn: &Connection,
    attachment_id: &str,
) -> VaultResult<()> {
    let attachment = load_attachment(conn, attachment_id)?;
    conn.execute(
        "DELETE FROM attachments WHERE id = ?1",
        params![attachment_id],
    )?;
    blob_store::remove_if_unref(files_dir, conn, &attachment.hash)
}

/// 页面全部附件（彻底删除页面时由 command 层逐个清理文件）。
pub fn list_attachments(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> VaultResult<Vec<Attachment>> {
    let mut stmt = conn.prepare(
        "SELECT id, entity_type, entity_id, file_name, mime, size, hash, created_at FROM attachments WHERE entity_type = ?1 AND entity_id = ?2 ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map(params![entity_type, entity_id], row_attachment)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 彻底删除页面时的附件清理（连文件一起，引用计数归零才删）。
pub fn purge_attachments_of_page(
    files_dir: &Path,
    conn: &Connection,
    page_id: &str,
) -> VaultResult<()> {
    let attachments = list_attachments(conn, "page", page_id)?;
    for attachment in attachments {
        delete_attachment(files_dir, conn, &attachment.id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::hierarchy;
    use super::super::sections_crypto;
    use super::*;
    use crate::crypto::kdf::KdfParams;
    use crate::db::migrate::{run_migrations, DbKind};

    fn fast_params() -> KdfParams {
        KdfParams {
            m_cost: 8 * 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    struct Fx {
        dir: tempfile::TempDir,
        conn: Connection,
        session: SessionManager,
        section_id: String,
        page_id: String,
    }

    fn files_dir(dir: &Path, space_id: &str) -> PathBuf {
        dir.join(format!("{space_id}.files"))
    }

    fn fixture(encrypted: bool) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        let nb = hierarchy::create_notebook(&conn, "nb", None).unwrap();
        let sec = hierarchy::create_section(&conn, &nb.id, None, "s", None).unwrap();
        let page = hierarchy::create_page(&conn, &sec.id, None, "p").unwrap();
        let session = SessionManager::new();
        if encrypted {
            let keys = sections_crypto::set_password_with_params(
                &mut conn,
                &sec.id,
                "pw-123",
                true,
                &fast_params(),
            )
            .unwrap();
            session.insert(&sec.id, keys);
        }
        Fx {
            dir,
            conn,
            session,
            section_id: sec.id,
            page_id: page.id,
        }
    }

    #[test]
    fn content_addressed_dedup_and_refcounted_delete() {
        let fx = fixture(false);
        let dir = fx.dir.path();
        let bytes = b"shared file bytes";
        let a1 = save_attachment(
            &files_dir(dir, "sp1"),
            &fx.conn,
            &fx.session,
            &fx.section_id,
            "page",
            &fx.page_id,
            "a.txt",
            Some("text/plain"),
            bytes,
        )
        .unwrap();
        let a2 = save_attachment(
            &files_dir(dir, "sp1"),
            &fx.conn,
            &fx.session,
            &fx.section_id,
            "page",
            &fx.page_id,
            "b.txt",
            Some("text/plain"),
            bytes,
        )
        .unwrap();
        assert_eq!(a1.hash, a2.hash, "同内容必须得到同哈希");
        assert!(files_dir(dir, "sp1").join(&a1.hash).exists());
        // 两次登记但物理文件只有一个
        assert_eq!(
            list_attachments(&fx.conn, "page", &fx.page_id)
                .unwrap()
                .len(),
            2
        );

        // 删一个：文件保留（仍有引用）
        delete_attachment(&files_dir(dir, "sp1"), &fx.conn, &a1.id).unwrap();
        assert!(
            files_dir(dir, "sp1").join(&a1.hash).exists(),
            "引用未归零不得删文件"
        );
        // 删第二个：引用归零，物理文件移除
        delete_attachment(&files_dir(dir, "sp1"), &fx.conn, &a2.id).unwrap();
        assert!(
            !files_dir(dir, "sp1").join(&a2.hash).exists(),
            "引用归零后物理文件必须移除"
        );
    }

    #[test]
    fn encrypted_attachment_stored_ciphertext_and_locked_rejected() {
        let mut fx = fixture(true);
        let dir = fx.dir.path();
        let att = save_attachment(
            &files_dir(dir, "sp1"),
            &fx.conn,
            &fx.session,
            &fx.section_id,
            "page",
            &fx.page_id,
            "secret.txt",
            None,
            b"top secret",
        )
        .unwrap();
        let stored = std::fs::read(files_dir(dir, "sp1").join(&att.hash)).unwrap();
        assert_ne!(stored, b"top secret", "加密分区附件必须密文落盘");

        // 解锁可读
        let data = open_attachment(&files_dir(dir, "sp1"), &fx.conn, &fx.session, &att.id).unwrap();
        assert_eq!(
            crate::notes::base64_decode(&data.data_base64).unwrap(),
            b"top secret"
        );

        // 锁定后不可打开（SectionLocked 而非乱码）
        fx.session.lock(&fx.section_id);
        assert!(matches!(
            open_attachment(&files_dir(dir, "sp1"), &fx.conn, &fx.session, &att.id),
            Err(VaultError::SectionLocked(_))
        ));
        // 锁定时也禁止写入
        assert!(matches!(
            save_attachment(
                &files_dir(dir, "sp1"),
                &fx.conn,
                &fx.session,
                &fx.section_id,
                "page",
                &fx.page_id,
                "x",
                None,
                b"y"
            ),
            Err(VaultError::SectionLocked(_))
        ));
        let _ = &mut fx.conn;
    }
}
