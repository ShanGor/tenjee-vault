//! 任务标签（spec: 任务标签与附件）。tag_id 跨库引用 meta.db 的 tags 表（仅存 ID，
//! 无跨库外键；标签字典被删后任务侧保持原样，展示层降级处理）。

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::{VaultError, VaultResult};

/// 任务的标签 id 列表（meta.db tags 表主键）。
pub fn get_task_tags(conn: &Connection, task_id: &str) -> VaultResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT tag_id FROM taggings WHERE entity_type = 'task' AND entity_id = ?1 ORDER BY tag_id",
    )?;
    let rows = stmt
        .query_map(params![task_id], |r| r.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 覆盖式设置任务标签（前端勾选后整组提交）。
pub fn set_task_tags(conn: &Connection, task_id: &str, tag_ids: &[String]) -> VaultResult<()> {
    if !crate::tasks::tasks::task_exists(conn, task_id)? {
        return Err(VaultError::NotFound(format!("任务 {task_id}")));
    }
    crate::tags::set_entity_tags(conn, "task", task_id, tag_ids)
}

/// 按标签筛选任务（返回任务 id；标签失效即 meta.db 无对应行时此处照常返回，
/// 名称解析由 command 层对 meta.db 做存在性检查后降级）。
#[derive(Debug, Clone, Serialize)]
pub struct TaggedTask {
    pub id: String,
    pub title: String,
    pub status: String,
    pub due_date: Option<String>,
}

pub fn tasks_by_tag(conn: &Connection, tag_id: &str) -> VaultResult<Vec<TaggedTask>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.status, t.due_date
         FROM tasks t
         JOIN taggings g ON g.entity_type = 'task' AND g.entity_id = t.id
         WHERE g.tag_id = ?1 AND t.archived_at IS NULL
         ORDER BY t.due_date IS NULL, t.due_date, t.created_at",
    )?;
    let rows = stmt
        .query_map(params![tag_id], |r| {
            Ok(TaggedTask {
                id: r.get(0)?,
                title: r.get(1)?,
                status: r.get(2)?,
                due_date: r.get(3)?,
            })
        })?
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
    fn set_filter_and_missing_tag_degrades() {
        let conn = conn();
        let list = crate::tasks::lists::create_list(&conn, "收件箱", None).unwrap();
        let t1 = crate::tasks::tasks::create_task(&conn, &list.id, None, "任务一").unwrap();
        let t2 = crate::tasks::tasks::create_task(&conn, &list.id, None, "任务二").unwrap();
        set_task_tags(&conn, &t1.id, &["tag-a".to_string(), "tag-b".to_string()]).unwrap();
        set_task_tags(&conn, &t2.id, &["tag-a".to_string()]).unwrap();

        assert_eq!(get_task_tags(&conn, &t1.id).unwrap().len(), 2);
        let by_a = tasks_by_tag(&conn, "tag-a").unwrap();
        assert_eq!(by_a.len(), 2);
        let by_b = tasks_by_tag(&conn, "tag-b").unwrap();
        assert_eq!(by_b.len(), 1);
        assert_eq!(by_b[0].id, t1.id);

        // 覆盖式更新
        set_task_tags(&conn, &t1.id, &["tag-c".to_string()]).unwrap();
        assert_eq!(get_task_tags(&conn, &t1.id).unwrap(), vec!["tag-c"]);
        assert!(tasks_by_tag(&conn, "tag-b").unwrap().is_empty());

        // 标签字典（meta.db）中不存在 tag-c 也不影响任务侧查询（失效降级：不报错）
        let _ = tasks_by_tag(&conn, "tag-c").unwrap();
    }
}
