//! 任务附件（spec: 任务标签与附件；design D8）：内容寻址存 `tasks.files/`，引用计数删除。
//! 与笔记附件不同：任务无加密语义，字节原样落盘。

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::new_id;
use crate::blob_store;
use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, Serialize)]
pub struct TaskAttachment {
    pub id: String,
    pub file_name: String,
    pub mime: Option<String>,
    pub size: i64,
    pub hash: String,
    pub created_at: String,
}

fn row_attachment(r: &rusqlite::Row<'_>) -> rusqlite::Result<TaskAttachment> {
    Ok(TaskAttachment {
        id: r.get(0)?,
        file_name: r.get(1)?,
        mime: r.get(2)?,
        size: r.get(3)?,
        hash: r.get(4)?,
        created_at: r.get(5)?,
    })
}

fn load_attachment(conn: &Connection, id: &str) -> VaultResult<(TaskAttachment, String)> {
    conn.query_row(
        "SELECT id, file_name, mime, size, hash, created_at, entity_id FROM attachments WHERE id = ?1 AND entity_type = 'task'",
        params![id],
        |r| {
            Ok((
                TaskAttachment {
                    id: r.get(0)?,
                    file_name: r.get(1)?,
                    mime: r.get(2)?,
                    size: r.get(3)?,
                    hash: r.get(4)?,
                    created_at: r.get(5)?,
                },
                r.get::<_, String>(6)?,
            ))
        },
    )
    .optional()?
    .ok_or_else(|| VaultError::NotFound(format!("任务附件 {id}")))
}

/// 保存附件：base64 明文 → 内容寻址落盘 + 登记。
pub fn save_attachment(
    files_dir: &Path,
    conn: &Connection,
    task_id: &str,
    file_name: &str,
    mime: Option<&str>,
    bytes: &[u8],
) -> VaultResult<TaskAttachment> {
    if bytes.is_empty() {
        return Err(VaultError::Validation("附件内容为空".into()));
    }
    if !crate::tasks::tasks::task_exists(conn, task_id)? {
        return Err(VaultError::NotFound(format!("任务 {task_id}")));
    }
    let hash = blob_store::store(files_dir, bytes)?;
    let att = TaskAttachment {
        id: new_id(),
        file_name: file_name.to_string(),
        mime: mime.map(str::to_string),
        size: bytes.len() as i64,
        hash,
        created_at: String::new(),
    };
    conn.execute(
        "INSERT INTO attachments (id, entity_type, entity_id, file_name, mime, size, hash) VALUES (?1, 'task', ?2, ?3, ?4, ?5, ?6)",
        params![att.id, task_id, att.file_name, att.mime, att.size, att.hash],
    )?;
    Ok(att)
}

/// 打开附件：读盘返回 base64 明文。
pub fn open_attachment(
    files_dir: &Path,
    conn: &Connection,
    id: &str,
) -> VaultResult<(TaskAttachment, String)> {
    let (att, _) = load_attachment(conn, id)?;
    let bytes = blob_store::read(files_dir, &att.hash)?;
    Ok((att, crate::notes::base64_encode(&bytes)))
}

/// 删除附件：删登记行；同哈希引用计数归零物理移除。
pub fn delete_attachment(files_dir: &Path, conn: &Connection, id: &str) -> VaultResult<()> {
    let (att, _) = load_attachment(conn, id)?;
    conn.execute("DELETE FROM attachments WHERE id = ?1", params![id])?;
    blob_store::remove_if_unref(files_dir, conn, &att.hash)
}

pub fn list_attachments(conn: &Connection, task_id: &str) -> VaultResult<Vec<TaskAttachment>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_name, mime, size, hash, created_at FROM attachments WHERE entity_type = 'task' AND entity_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map(params![task_id], row_attachment)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate::{run_migrations, DbKind};

    fn conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Tasks.migrations()).unwrap();
        conn
    }

    #[test]
    fn dedup_and_refcounted_delete() {
        let dir = tempfile::tempdir().unwrap();
        let conn = conn();
        let list = crate::tasks::lists::create_list(&conn, "收件箱", None).unwrap();
        let t = crate::tasks::tasks::create_task(&conn, &list.id, None, "带附件").unwrap();
        let bytes = b"same bytes";
        let a1 = save_attachment(dir.path(), &conn, &t.id, "a.bin", None, bytes).unwrap();
        let a2 = save_attachment(
            dir.path(),
            &conn,
            &t.id,
            "b.bin",
            Some("application/octet-stream"),
            bytes,
        )
        .unwrap();
        assert_eq!(a1.hash, a2.hash, "同内容同哈希");
        assert!(blob_store::blob_path(dir.path(), &a1.hash).exists());
        assert_eq!(list_attachments(&conn, &t.id).unwrap().len(), 2);

        delete_attachment(dir.path(), &conn, &a1.id).unwrap();
        assert!(
            blob_store::blob_path(dir.path(), &a1.hash).exists(),
            "引用未归零不得删文件"
        );
        delete_attachment(dir.path(), &conn, &a2.id).unwrap();
        assert!(
            !blob_store::blob_path(dir.path(), &a2.hash).exists(),
            "引用归零后物理文件移除"
        );
        assert!(list_attachments(&conn, &t.id).unwrap().is_empty());
    }

    #[test]
    fn open_returns_base64() {
        let dir = tempfile::tempdir().unwrap();
        let conn = conn();
        let list = crate::tasks::lists::create_list(&conn, "收件箱", None).unwrap();
        let t = crate::tasks::tasks::create_task(&conn, &list.id, None, "任务").unwrap();
        let att = save_attachment(
            dir.path(),
            &conn,
            &t.id,
            "x.txt",
            Some("text/plain"),
            b"hello",
        )
        .unwrap();
        let (_, data) = open_attachment(dir.path(), &conn, &att.id).unwrap();
        assert_eq!(crate::notes::base64_decode(&data).unwrap(), b"hello");
    }
}
