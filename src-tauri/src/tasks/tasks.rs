//! 任务核心（spec: 任务层级与子任务 / 任务属性与状态流转 / 重复任务 /
//! 任务视图 / 任务排序、批量操作与快速添加 / 已完成任务归档与恢复）。
//!
//! 重复任务语义（design D2/D3 的实例化变体）：规则存于 `recurrence_rule`，
//! 完成时按规则从当前截止日推算下一实例（跳过已过期实例直到未来），
//! 生成继承属性的新任务行；删除任务行即终止后续生成。

use std::path::Path;

use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;

use super::new_id;
use crate::calendar::rrule::RRule;
use crate::error::{VaultError, VaultResult};

/// 状态（对齐 tasks.status CHECK 约束）。
pub const STATUS_TODO: &str = "todo";
pub const STATUS_IN_PROGRESS: &str = "in_progress";
pub const STATUS_DONE: &str = "done";
pub const STATUS_CANCELLED: &str = "cancelled";

/// 优先级（对齐 tasks.priority CHECK 约束）。
pub const PRIORITY_NONE: &str = "none";
pub const PRIORITY_LOW: &str = "low";
pub const PRIORITY_MEDIUM: &str = "medium";
pub const PRIORITY_HIGH: &str = "high";

const ACTIVE_STATUSES: &str = "('todo', 'in_progress')";

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub list_id: String,
    pub title: String,
    pub notes: Option<String>,
    pub status: String,
    pub priority: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub reminder_at: Option<String>,
    pub recurrence_rule: Option<String>,
    pub parent_task_id: Option<String>,
    pub sort_order: i64,
    pub completed_at: Option<String>,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskNode {
    #[serde(flatten)]
    pub task: Task,
    pub children: Vec<TaskNode>,
}

/// 可编辑字段补丁（None = 不修改）。
#[derive(Debug, Clone, Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub notes: Option<Option<String>>,
    pub priority: Option<String>,
    pub due_date: Option<Option<String>>,
    pub due_time: Option<Option<String>>,
    pub reminder_at: Option<Option<String>>,
    pub recurrence_rule: Option<Option<String>>,
    pub list_id: Option<String>,
    pub parent_task_id: Option<Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartView {
    Today,
    Week,
    Overdue,
}

#[derive(Debug, Clone)]
pub enum BatchAction {
    Complete,
    Cancel,
    Delete,
    MoveToList(String),
}

fn row_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        list_id: r.get(1)?,
        title: r.get(2)?,
        notes: r.get(3)?,
        status: r.get(4)?,
        priority: r.get(5)?,
        due_date: r.get(6)?,
        due_time: r.get(7)?,
        reminder_at: r.get(8)?,
        recurrence_rule: r.get(9)?,
        parent_task_id: r.get(10)?,
        sort_order: r.get(11)?,
        completed_at: r.get(12)?,
        archived_at: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
    })
}

const SELECT_TASK: &str = "SELECT id, list_id, title, notes, status, priority, due_date, due_time, reminder_at, recurrence_rule, parent_task_id, sort_order, completed_at, archived_at, created_at, updated_at FROM tasks";

fn load_task(conn: &Connection, id: &str) -> VaultResult<Task> {
    conn.query_row(
        &format!("{SELECT_TASK} WHERE id = ?1"),
        params![id],
        row_task,
    )
    .optional()?
    .ok_or_else(|| VaultError::NotFound(format!("任务 {id}")))
}

fn validate_priority(priority: &str) -> VaultResult<()> {
    match priority {
        PRIORITY_NONE | PRIORITY_LOW | PRIORITY_MEDIUM | PRIORITY_HIGH => Ok(()),
        other => Err(VaultError::Validation(format!("优先级 {other} 非法"))),
    }
}

/// 快速添加/新建任务：仅需列表与标题，其余默认。
pub fn create_task(
    conn: &Connection,
    list_id: &str,
    parent_task_id: Option<&str>,
    title: &str,
) -> VaultResult<Task> {
    let title = title.trim();
    if title.is_empty() {
        return Err(VaultError::Validation("任务标题不能为空".into()));
    }
    if let Some(parent) = parent_task_id {
        if !task_exists(conn, parent)? {
            return Err(VaultError::NotFound(format!("父任务 {parent}")));
        }
    }
    let max: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM tasks WHERE list_id = ?1 AND parent_task_id IS ?2",
            params![list_id, parent_task_id],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    let task = Task {
        id: new_id(),
        list_id: list_id.to_string(),
        title: title.to_string(),
        notes: None,
        status: STATUS_TODO.to_string(),
        priority: PRIORITY_NONE.to_string(),
        due_date: None,
        due_time: None,
        reminder_at: None,
        recurrence_rule: None,
        parent_task_id: parent_task_id.map(str::to_string),
        sort_order: max + 1,
        completed_at: None,
        archived_at: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    conn.execute(
        "INSERT INTO tasks (id, list_id, title, priority, parent_task_id, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![task.id, task.list_id, task.title, task.priority, task.parent_task_id, task.sort_order],
    )?;
    Ok(task)
}

pub(crate) fn task_exists(conn: &Connection, id: &str) -> VaultResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 编辑任务属性（spec: 任务属性与状态流转）。移动列表/父任务/排序也走这里或专用函数。
pub fn update_task(conn: &mut Connection, id: &str, patch: TaskPatch) -> VaultResult<Task> {
    if let Some(title) = &patch.title {
        if title.trim().is_empty() {
            return Err(VaultError::Validation("任务标题不能为空".into()));
        }
    }
    if let Some(p) = &patch.priority {
        validate_priority(p)?;
    }
    if let Some(Some(rule)) = &patch.recurrence_rule {
        RRule::parse(rule)?;
    }
    let tx = conn.unchecked_transaction()?;
    if let Some(title) = &patch.title {
        tx.execute(
            "UPDATE tasks SET title = ?1 WHERE id = ?2",
            params![title, id],
        )?;
    }
    if let Some(notes) = &patch.notes {
        tx.execute(
            "UPDATE tasks SET notes = ?1 WHERE id = ?2",
            params![notes, id],
        )?;
    }
    if let Some(priority) = &patch.priority {
        tx.execute(
            "UPDATE tasks SET priority = ?1 WHERE id = ?2",
            params![priority, id],
        )?;
    }
    if let Some(due_date) = &patch.due_date {
        tx.execute(
            "UPDATE tasks SET due_date = ?1 WHERE id = ?2",
            params![due_date, id],
        )?;
    }
    if let Some(due_time) = &patch.due_time {
        tx.execute(
            "UPDATE tasks SET due_time = ?1 WHERE id = ?2",
            params![due_time, id],
        )?;
    }
    if let Some(reminder_at) = &patch.reminder_at {
        tx.execute(
            "UPDATE tasks SET reminder_at = ?1 WHERE id = ?2",
            params![reminder_at, id],
        )?;
    }
    if let Some(rule) = &patch.recurrence_rule {
        tx.execute(
            "UPDATE tasks SET recurrence_rule = ?1 WHERE id = ?2",
            params![rule, id],
        )?;
    }
    if let Some(list_id) = &patch.list_id {
        tx.execute(
            "UPDATE tasks SET list_id = ?1 WHERE id = ?2",
            params![list_id, id],
        )?;
    }
    if let Some(parent) = &patch.parent_task_id {
        if let Some(p) = parent {
            if p == id {
                return Err(VaultError::Validation("任务不能作为自己的子任务".into()));
            }
            if !task_exists(&tx, p)? {
                return Err(VaultError::NotFound(format!("父任务 {p}")));
            }
            if is_descendant(&tx, id, p)? {
                return Err(VaultError::Validation(
                    "不能将任务移动到自己的子任务下".into(),
                ));
            }
        }
        tx.execute(
            "UPDATE tasks SET parent_task_id = ?1 WHERE id = ?2",
            params![parent, id],
        )?;
    }
    let n = tx.execute(
        "UPDATE tasks SET updated_at = datetime('now') WHERE id = ?1",
        params![id],
    )?;
    tx.commit()?;
    if n == 0 {
        return Err(VaultError::NotFound(format!("任务 {id}")));
    }
    load_task(conn, id)
}

/// id 是否为 ancestor 的后代（防环）。
fn is_descendant(conn: &Connection, ancestor: &str, id: &str) -> VaultResult<bool> {
    let n: i64 = conn.query_row(
        "WITH RECURSIVE sub(id) AS (
            SELECT id FROM tasks WHERE id = ?2
            UNION ALL
            SELECT t.id FROM tasks t JOIN sub s ON t.parent_task_id = s.id
        ) SELECT COUNT(*) FROM sub WHERE id = ?1",
        params![ancestor, id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 状态流转（spec: 状态流转记录完成时间）。
/// 完成走 `complete_task`（含重复重生）；其余状态在此处理。
pub fn set_task_status(conn: &mut Connection, id: &str, status: &str) -> VaultResult<()> {
    match status {
        STATUS_DONE => {
            complete_task(conn, id)?;
            Ok(())
        }
        STATUS_TODO | STATUS_IN_PROGRESS | STATUS_CANCELLED => {
            let completed_sql = if status == STATUS_CANCELLED {
                "datetime('now')"
            } else {
                "NULL"
            };
            let n = conn.execute(
                &format!("UPDATE tasks SET status = ?1, completed_at = {completed_sql}, updated_at = datetime('now') WHERE id = ?2"),
                params![status, id],
            )?;
            if n == 0 {
                return Err(VaultError::NotFound(format!("任务 {id}")));
            }
            Ok(())
        }
        other => Err(VaultError::Validation(format!("状态 {other} 非法"))),
    }
}

/// 完成任务：记录完成时间；带重复规则时生成下一实例（继承属性，截止按规则推移；
/// 已过期实例连续跳过直到未来）。返回生成的新任务（如有）。
pub fn complete_task(conn: &mut Connection, id: &str) -> VaultResult<Option<Task>> {
    let task = load_task(conn, id)?;
    if task.status == STATUS_DONE {
        return Ok(None);
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE tasks SET status = 'done', completed_at = datetime('now'), updated_at = datetime('now') WHERE id = ?1",
        params![id],
    )?;
    let spawned = if let Some(rule_str) = &task.recurrence_rule {
        let rule = RRule::parse(rule_str)?;
        let today = chrono::Utc::now().date_naive();
        let mut base = task
            .due_date
            .as_deref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .unwrap_or(today);
        let mut next = rule.next_after(base);
        while let Some(cand) = next {
            if cand > today {
                break;
            }
            base = cand;
            next = rule.next_after(base);
        }
        match next {
            Some(next_due) => Some(spawn_next_instance(&tx, &task, next_due)?),
            None => None,
        }
    } else {
        None
    };
    tx.commit()?;
    Ok(spawned)
}

/// 生成重复任务的下一实例（事务内）：继承标题/备注/优先级/截止时刻/重复规则/
/// 列表/父任务/标签；提醒按截止位移整体平移；状态归零。
fn spawn_next_instance(
    tx: &Transaction<'_>,
    task: &Task,
    next_due: NaiveDate,
) -> VaultResult<Task> {
    let next_due_str = next_due.format("%Y-%m-%d").to_string();
    let reminder_at = shift_reminder(task, &next_due_str);
    let new_id = new_id();
    let max: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM tasks WHERE list_id = ?1 AND parent_task_id IS ?2",
            params![task.list_id, task.parent_task_id],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    tx.execute(
        "INSERT INTO tasks (id, list_id, title, notes, priority, due_date, due_time, reminder_at, recurrence_rule, parent_task_id, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            new_id,
            task.list_id,
            task.title,
            task.notes,
            task.priority,
            next_due_str,
            task.due_time,
            reminder_at,
            task.recurrence_rule,
            task.parent_task_id,
            max + 1
        ],
    )?;
    // 继承标签
    let tag_ids: Vec<String> = {
        let mut stmt = tx
            .prepare("SELECT tag_id FROM taggings WHERE entity_type = 'task' AND entity_id = ?1")?;
        let rows = stmt
            .query_map(params![task.id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    for tag_id in tag_ids {
        tx.execute(
            "INSERT INTO taggings (tag_id, entity_type, entity_id) VALUES (?1, 'task', ?2)",
            params![tag_id, new_id],
        )?;
    }
    Ok(load_task(tx, &new_id)?)
}

/// 提醒随截止日整体平移（无截止或无提醒则不设）。
fn shift_reminder(task: &Task, next_due: &str) -> Option<String> {
    let old_due = task.due_date.as_deref()?;
    let reminder = task.reminder_at.as_deref()?;
    let old_d = NaiveDate::parse_from_str(old_due, "%Y-%m-%d").ok()?;
    let new_d = NaiveDate::parse_from_str(next_due, "%Y-%m-%d").ok()?;
    let delta = new_d - old_d;
    let rt = chrono::NaiveDateTime::parse_from_str(reminder, "%Y-%m-%dT%H:%M:%S").ok()?;
    Some((rt + delta).format("%Y-%m-%dT%H:%M:%S").to_string())
}

/// 删除任务：级联全部后代（含多级子任务），清理附件（文件引用计数归零才删）与标签（事务）。
pub fn delete_task(conn: &mut Connection, files_dir: &Path, id: &str) -> VaultResult<()> {
    let tx = conn.unchecked_transaction()?;
    delete_task_in_tx(&tx, files_dir, id)?;
    tx.commit()?;
    Ok(())
}

/// `delete_task` 的事务内版本（列表级联删除复用）。
pub(crate) fn delete_task_in_tx(
    tx: &Transaction<'_>,
    files_dir: &Path,
    id: &str,
) -> VaultResult<()> {
    let ids: Vec<String> = tx
        .prepare(
            "WITH RECURSIVE sub(id) AS (
                SELECT id FROM tasks WHERE id = ?1
                UNION ALL
                SELECT t.id FROM tasks t JOIN sub s ON t.parent_task_id = s.id
            ) SELECT id FROM sub",
        )?
        .query_map(params![id], |r| r.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for tid in &ids {
        // 附件：删登记行 + 引用计数归零清文件
        let atts: Vec<(String, String)> = {
            let mut stmt = tx.prepare(
                "SELECT id, hash FROM attachments WHERE entity_type = 'task' AND entity_id = ?1",
            )?;
            let rows = stmt
                .query_map(params![tid], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        for (att_id, hash) in atts {
            tx.execute("DELETE FROM attachments WHERE id = ?1", params![att_id])?;
            crate::blob_store::remove_if_unref(files_dir, tx, &hash)?;
        }
        tx.execute(
            "DELETE FROM taggings WHERE entity_type = 'task' AND entity_id = ?1",
            params![tid],
        )?;
    }
    // 先删后代再删本体（外键自引用）
    let sql = format!(
        "DELETE FROM tasks WHERE id IN ({})",
        ids.iter().map(|_| "?").collect::<Vec<_>>().join(",")
    );
    tx.execute(&sql, rusqlite::params_from_iter(ids.iter()))?;
    Ok(())
}

/// 拖拽排序（列表内，可能换父级）。
pub fn reorder_task(
    conn: &Connection,
    id: &str,
    new_parent: Option<&str>,
    new_sort_order: i64,
) -> VaultResult<()> {
    let n = conn.execute(
        "UPDATE tasks SET parent_task_id = ?1, sort_order = ?2, updated_at = datetime('now') WHERE id = ?3",
        params![new_parent, new_sort_order, id],
    )?;
    if n == 0 {
        return Err(VaultError::NotFound(format!("任务 {id}")));
    }
    Ok(())
}

/// 列表视图：某列表的全部未归档任务树（按 sort_order；父任务缺失时按顶层处理）。
pub fn list_view(conn: &Connection, list_id: &str) -> VaultResult<Vec<TaskNode>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT_TASK} WHERE list_id = ?1 AND archived_at IS NULL ORDER BY sort_order"
    ))?;
    let tasks: Vec<Task> = stmt
        .query_map(params![list_id], row_task)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(build_tree(tasks))
}

/// 看板视图：全部未归档任务（前端按状态分列）。
pub fn kanban_view(conn: &Connection) -> VaultResult<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT_TASK} WHERE archived_at IS NULL ORDER BY sort_order"
    ))?;
    let rows = stmt
        .query_map([], row_task)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 智能视图（今日/本周/逾期；排除归档与已完成，按截止排序）。
/// 本周按周一为一周之始（周起始默认周一，见 design D9）。
pub fn smart_view(conn: &Connection, view: SmartView) -> VaultResult<Vec<Task>> {
    let cond = match view {
        SmartView::Today => {
            format!("archived_at IS NULL AND status IN {ACTIVE_STATUSES} AND due_date = date('now')")
        }
        SmartView::Week => format!(
            "archived_at IS NULL AND status IN {ACTIVE_STATUSES} AND due_date BETWEEN date('now', '-6 days', 'weekday 1') AND date('now', '-6 days', 'weekday 1', '+6 days')"
        ),
        SmartView::Overdue => {
            format!("archived_at IS NULL AND status IN {ACTIVE_STATUSES} AND due_date < date('now')")
        }
    };
    let mut stmt = conn.prepare(&format!(
        "{SELECT_TASK} WHERE {cond} ORDER BY due_date IS NULL, due_date, due_time IS NULL, due_time"
    ))?;
    let rows = stmt
        .query_map([], row_task)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 归档视图：全部已归档任务（新→旧按归档时间）。
pub fn list_archived(conn: &Connection) -> VaultResult<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT_TASK} WHERE archived_at IS NOT NULL ORDER BY archived_at DESC"
    ))?;
    let rows = stmt
        .query_map([], row_task)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 归档任务（spec: 已完成任务归档与恢复）。
pub fn archive_tasks(conn: &Connection, ids: &[String]) -> VaultResult<()> {
    for id in ids {
        let n = conn.execute(
            "UPDATE tasks SET archived_at = datetime('now'), updated_at = datetime('now') WHERE id = ?1 AND archived_at IS NULL",
            params![id],
        )?;
        if n == 0 {
            return Err(VaultError::NotFound(format!("任务 {id}")));
        }
    }
    Ok(())
}

/// 恢复归档任务（保持完成状态；completed_at 不动）。
pub fn unarchive_task(conn: &Connection, id: &str) -> VaultResult<()> {
    let n = conn.execute(
        "UPDATE tasks SET archived_at = NULL, updated_at = datetime('now') WHERE id = ?1",
        params![id],
    )?;
    if n == 0 {
        return Err(VaultError::NotFound(format!("任务 {id}")));
    }
    Ok(())
}

/// 彻底删除（归档视图内物理删除）。
pub fn purge_task(conn: &mut Connection, files_dir: &Path, id: &str) -> VaultResult<()> {
    if load_task(conn, id).is_err() {
        return Err(VaultError::NotFound(format!("任务 {id}")));
    }
    delete_task(conn, files_dir, id)
}

/// 批量操作（spec: 批量操作）。
pub fn batch(
    conn: &mut Connection,
    files_dir: &Path,
    ids: &[String],
    action: BatchAction,
) -> VaultResult<()> {
    match action {
        BatchAction::Complete => {
            for id in ids {
                complete_task(conn, id)?;
            }
        }
        BatchAction::Cancel => {
            for id in ids {
                set_task_status(conn, id, STATUS_CANCELLED)?;
            }
        }
        BatchAction::Delete => {
            for id in ids {
                delete_task(conn, files_dir, id)?;
            }
        }
        BatchAction::MoveToList(list_id) => {
            for id in ids {
                let n = conn.execute(
                    "UPDATE tasks SET list_id = ?1, updated_at = datetime('now') WHERE id = ?2",
                    params![list_id, id],
                )?;
                if n == 0 {
                    return Err(VaultError::NotFound(format!("任务 {id}")));
                }
            }
        }
    }
    Ok(())
}

/// 平铺任务表建树（父缺失/不在集合内视为顶层；兄弟保持输入顺序）。
fn build_tree(tasks: Vec<Task>) -> Vec<TaskNode> {
    use std::collections::{HashMap, HashSet};
    let ids: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    let mut children_map: HashMap<String, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for (i, t) in tasks.iter().enumerate() {
        match &t.parent_task_id {
            Some(p) if ids.contains(p.as_str()) => {
                children_map.entry(p.clone()).or_default().push(i)
            }
            _ => roots.push(i),
        }
    }
    let mut taken: Vec<Option<Task>> = tasks.into_iter().map(Some).collect();
    fn assemble(
        idx: usize,
        taken: &mut Vec<Option<Task>>,
        children_map: &HashMap<String, Vec<usize>>,
    ) -> TaskNode {
        let task = taken[idx].take().expect("节点唯一组装");
        let children = children_map
            .get(&task.id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|ci| assemble(ci, taken, children_map))
            .collect();
        TaskNode { task, children }
    }
    roots
        .into_iter()
        .map(|i| assemble(i, &mut taken, &children_map))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    fn conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        crate::db::migrate::run_migrations(
            &mut conn,
            crate::db::migrate::DbKind::Tasks.migrations(),
        )
        .unwrap();
        conn
    }

    #[test]
    fn status_flow_records_and_clears_completed_at() {
        let mut conn = conn();
        let list = super::super::lists::create_list(&conn, "收件箱", None).unwrap();
        let t = create_task(&conn, &list.id, None, "任务A").unwrap();
        complete_task(&mut conn, &t.id).unwrap();
        let done = load_task(&conn, &t.id).unwrap();
        assert_eq!(done.status, STATUS_DONE);
        assert!(done.completed_at.is_some(), "完成应记录完成时间");
        set_task_status(&mut conn, &t.id, STATUS_TODO).unwrap();
        let back = load_task(&conn, &t.id).unwrap();
        assert_eq!(back.status, STATUS_TODO);
        assert!(back.completed_at.is_none(), "改回待办应清空完成时间");
    }

    #[test]
    fn delete_parent_cascades_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = conn();
        let list = super::super::lists::create_list(&conn, "收件箱", None).unwrap();
        let p = create_task(&conn, &list.id, None, "父").unwrap();
        let c = create_task(&conn, &list.id, Some(&p.id), "子").unwrap();
        let g = create_task(&conn, &list.id, Some(&c.id), "孙").unwrap();
        delete_task(&mut conn, dir.path(), &p.id).unwrap();
        for id in [&p.id, &c.id, &g.id] {
            assert!(!task_exists(&conn, id).unwrap(), "{id} 应被级联删除");
        }
    }

    #[test]
    fn recurring_task_respawns_on_complete() {
        let mut conn = conn();
        let list = super::super::lists::create_list(&conn, "收件箱", None).unwrap();
        let t = create_task(&conn, &list.id, None, "每周例会").unwrap();
        update_task(
            &mut conn,
            &t.id,
            TaskPatch {
                recurrence_rule: Some(Some("FREQ=WEEKLY".into())),
                ..Default::default()
            },
        )
        .unwrap();
        let spawned = complete_task(&mut conn, &t.id)
            .unwrap()
            .expect("应生成下一实例");
        assert_eq!(spawned.title, "每周例会");
        assert_eq!(spawned.status, STATUS_TODO);
        assert!(spawned.completed_at.is_none());
        // 下一实例截止：今天之后最近的一个周（同日）
        let today = chrono::Utc::now().date_naive();
        let next_due =
            NaiveDate::parse_from_str(spawned.due_date.as_deref().unwrap(), "%Y-%m-%d").unwrap();
        assert!(next_due > today, "下一实例应在未来");
        assert_eq!(next_due.weekday(), today.weekday(), "周重复保持同一星期");
    }

    #[test]
    fn deleting_recurring_task_stops_generation() {
        let mut conn = conn();
        let list = super::super::lists::create_list(&conn, "收件箱", None).unwrap();
        let t = create_task(&conn, &list.id, None, "重复").unwrap();
        update_task(
            &mut conn,
            &t.id,
            TaskPatch {
                recurrence_rule: Some(Some("FREQ=DAILY".into())),
                ..Default::default()
            },
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        delete_task(&mut conn, dir.path(), &t.id).unwrap();
        assert!(!task_exists(&conn, &t.id).unwrap());
        // 无重生（任务行已删）
    }

    #[test]
    fn batch_complete_records_all() {
        let mut conn = conn();
        let list = super::super::lists::create_list(&conn, "收件箱", None).unwrap();
        let t1 = create_task(&conn, &list.id, None, "一").unwrap();
        let t2 = create_task(&conn, &list.id, None, "二").unwrap();
        batch(
            &mut conn,
            tempfile::tempdir().unwrap().path(),
            &[t1.id.clone(), t2.id.clone()],
            BatchAction::Complete,
        )
        .unwrap();
        assert_eq!(load_task(&conn, &t1.id).unwrap().status, STATUS_DONE);
        assert_eq!(load_task(&conn, &t2.id).unwrap().status, STATUS_DONE);
    }

    #[test]
    fn archive_hides_and_restore_keeps_done() {
        let mut conn = conn();
        let list = super::super::lists::create_list(&conn, "收件箱", None).unwrap();
        let t = create_task(&conn, &list.id, None, "归档我").unwrap();
        complete_task(&mut conn, &t.id).unwrap();
        archive_tasks(&conn, &[t.id.clone()]).unwrap();
        assert!(
            list_view(&conn, &list.id).unwrap().is_empty(),
            "归档后常规视图不可见"
        );
        assert_eq!(list_archived(&conn).unwrap().len(), 1);
        unarchive_task(&conn, &t.id).unwrap();
        let restored = load_task(&conn, &t.id).unwrap();
        assert!(restored.archived_at.is_none());
        assert_eq!(restored.status, STATUS_DONE, "恢复保持完成状态");
    }

    #[test]
    fn smart_views_today_and_overdue() {
        let mut conn = conn();
        let list = super::super::lists::create_list(&conn, "收件箱", None).unwrap();
        let today = chrono::Utc::now().date_naive();
        let yesterday = today - chrono::Duration::days(1);
        let t_today = create_task(&conn, &list.id, None, "今天截止").unwrap();
        update_task(
            &mut conn,
            &t_today.id,
            TaskPatch {
                due_date: Some(Some(today.format("%Y-%m-%d").to_string())),
                ..Default::default()
            },
        )
        .unwrap();
        let t_over = create_task(&conn, &list.id, None, "昨天截止").unwrap();
        update_task(
            &mut conn,
            &t_over.id,
            TaskPatch {
                due_date: Some(Some(yesterday.format("%Y-%m-%d").to_string())),
                ..Default::default()
            },
        )
        .unwrap();
        let today_view = smart_view(&conn, SmartView::Today).unwrap();
        assert_eq!(today_view.len(), 1);
        assert_eq!(today_view[0].id, t_today.id);
        let overdue = smart_view(&conn, SmartView::Overdue).unwrap();
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].id, t_over.id);
        // 完成后退出智能视图
        complete_task(&mut conn, &t_over.id).unwrap();
        assert!(smart_view(&conn, SmartView::Overdue).unwrap().is_empty());
    }
}
