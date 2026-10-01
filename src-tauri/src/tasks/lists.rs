//! 任务列表管理（spec: 任务列表管理）。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::new_id;
use crate::error::{VaultError, VaultResult};

/// 默认收件箱列表名（首次进入任务模块自动创建）。
pub const INBOX_NAME: &str = "收件箱";

#[derive(Debug, Clone, Serialize)]
pub struct TaskList {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i64,
}

fn row_list(r: &rusqlite::Row<'_>) -> rusqlite::Result<TaskList> {
    Ok(TaskList {
        id: r.get(0)?,
        name: r.get(1)?,
        color: r.get(2)?,
        sort_order: r.get(3)?,
    })
}

fn must_update(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
    what: &str,
) -> VaultResult<()> {
    let n = conn.execute(sql, params)?;
    if n == 0 {
        return Err(VaultError::NotFound(what.into()));
    }
    Ok(())
}

/// 全部列表按排序升序。首次访问（无任何列表）自动创建默认收件箱。
pub fn list_lists(conn: &Connection) -> VaultResult<Vec<TaskList>> {
    ensure_inbox(conn)?;
    let mut stmt =
        conn.prepare("SELECT id, name, color, sort_order FROM task_lists ORDER BY sort_order, id")?;
    let lists = stmt
        .query_map([], row_list)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(lists)
}

/// 无任何列表时创建默认收件箱（幂等）。
pub fn ensure_inbox(conn: &Connection) -> VaultResult<()> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM task_lists", [], |r| r.get(0))?;
    if n == 0 {
        create_list(conn, INBOX_NAME, None)?;
    }
    Ok(())
}

pub fn create_list(conn: &Connection, name: &str, color: Option<&str>) -> VaultResult<TaskList> {
    let name = name.trim();
    if name.is_empty() {
        return Err(VaultError::Validation("列表名称不能为空".into()));
    }
    let max: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM task_lists",
            [],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    let list = TaskList {
        id: new_id(),
        name: name.to_string(),
        color: color.map(str::to_string),
        sort_order: max + 1,
    };
    conn.execute(
        "INSERT INTO task_lists (id, name, color, sort_order) VALUES (?1, ?2, ?3, ?4)",
        params![list.id, list.name, list.color, list.sort_order],
    )?;
    Ok(list)
}

pub fn rename_list(conn: &Connection, id: &str, name: &str) -> VaultResult<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(VaultError::Validation("列表名称不能为空".into()));
    }
    must_update(
        conn,
        "UPDATE task_lists SET name = ?1 WHERE id = ?2",
        &params![name, id],
        "任务列表",
    )
}

pub fn set_list_color(conn: &Connection, id: &str, color: Option<&str>) -> VaultResult<()> {
    must_update(
        conn,
        "UPDATE task_lists SET color = ?1 WHERE id = ?2",
        &params![color, id],
        "任务列表",
    )
}

/// 批量按给定 id 顺序重排。
pub fn reorder_lists(conn: &Connection, ids: &[String]) -> VaultResult<()> {
    for (i, id) in ids.iter().enumerate() {
        must_update(
            conn,
            "UPDATE task_lists SET sort_order = ?1 WHERE id = ?2",
            &params![i as i64, id],
            "任务列表",
        )?;
    }
    Ok(())
}

/// 删除列表。非空列表必须 `confirm_non_empty = true`（前端确认流程把关），
/// 确认后其中全部任务（含多级子任务，含附件文件）一并级联删除（事务）。
pub fn delete_list(
    conn: &Connection,
    files_dir: &std::path::Path,
    id: &str,
    confirm_non_empty: bool,
) -> VaultResult<()> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE list_id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    if count > 0 && !confirm_non_empty {
        return Err(VaultError::Validation(format!(
            "列表内仍有 {count} 个任务，删除需确认"
        )));
    }
    let tx = conn.unchecked_transaction()?;
    let task_ids: Vec<String> = {
        let mut stmt = tx.prepare("SELECT id FROM tasks WHERE list_id = ?1")?;
        let rows = stmt
            .query_map(params![id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    for task_id in &task_ids {
        super::tasks::delete_task_in_tx(&tx, files_dir, task_id)?;
    }
    tx.execute("DELETE FROM task_lists WHERE id = ?1", params![id])?;
    tx.commit()?;
    Ok(())
}

pub fn get_list(conn: &Connection, id: &str) -> VaultResult<TaskList> {
    conn.query_row(
        "SELECT id, name, color, sort_order FROM task_lists WHERE id = ?1",
        params![id],
        row_list,
    )
    .optional()?
    .ok_or_else(|| VaultError::NotFound(format!("任务列表 {id}")))
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
    fn inbox_created_on_first_access() {
        let conn = conn();
        let lists = list_lists(&conn).unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].name, INBOX_NAME, "首次访问应自动创建默认收件箱");
        // 幂等
        let again = list_lists(&conn).unwrap();
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn reorder_persists() {
        let conn = conn();
        ensure_inbox(&conn).unwrap();
        let a = create_list(&conn, "工作", Some("#f00")).unwrap();
        let b = create_list(&conn, "个人", None).unwrap();
        reorder_lists(&conn, &[b.id.clone(), a.id.clone()]).unwrap();
        let lists = list_lists(&conn).unwrap();
        let pos = |name: &str| lists.iter().position(|l| l.name == name).unwrap();
        assert!(pos("个人") < pos("工作"), "拖拽排序应持久化");
    }

    #[test]
    fn deleting_nonempty_list_requires_confirm_and_cascades() {
        let dir = tempfile::tempdir().unwrap();
        let conn = conn();
        ensure_inbox(&conn).unwrap();
        let list = create_list(&conn, "临时", None).unwrap();
        let parent = super::super::tasks::create_task(&conn, &list.id, None, "父任务").unwrap();
        super::super::tasks::create_task(&conn, &list.id, Some(&parent.id), "子任务").unwrap();

        // 未确认 → 拒绝
        assert!(matches!(
            delete_list(&conn, dir.path(), &list.id, false),
            Err(VaultError::Validation(_))
        ));
        // 确认 → 级联删除
        delete_list(&conn, dir.path(), &list.id, true).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE list_id = ?1",
                params![list.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "列表删除后任务应级联清空");
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 0, "父与子任务一并删除");
        assert!(get_list(&conn, &list.id).is_err());
        // 其他列表不受影响
        assert_eq!(list_lists(&conn).unwrap().len(), 1);
    }
}
