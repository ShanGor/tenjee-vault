//! 层级组织：空间内笔记本 / 分区组（可嵌套）/ 分区 / 页面的 CRUD、
//! 重命名、排序、拖拽移动与回收站（spec: 笔记本与分区层级组织 / 页面树管理 / 回收站）。
//!
//! 删除语义：删除笔记本 / 分区组 / 分区时，其下页面一律软删除进回收站
//! （`pages.is_deleted = 1`，数据保留可恢复），容器行在事务内物理删除。
//! 注：容器已删后，其回收站页面的「恢复」因原分区不存在而失败（明确报错），
//! 彻底删除（purge）不受影响。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::new_id;
use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, Serialize)]
pub struct Notebook {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SectionGroup {
    pub id: String,
    pub notebook_id: String,
    pub parent_group_id: Option<String>,
    pub name: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub id: String,
    pub notebook_id: String,
    pub section_group_id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i64,
    pub is_encrypted: bool,
    pub is_unlocked: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageSummary {
    pub id: String,
    pub section_id: String,
    pub parent_page_id: Option<String>,
    pub title: String,
    pub sort_order: i64,
    pub updated_at: String,
}

/// 导航树节点（笔记本 → 嵌套分区组 → 分区；分区 → 页面树）。
#[derive(Debug, Clone, Serialize)]
pub struct SectionGroupNode {
    #[serde(flatten)]
    pub group: SectionGroup,
    pub children: Vec<SectionGroupNode>,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotebookNode {
    #[serde(flatten)]
    pub notebook: Notebook,
    pub groups: Vec<SectionGroupNode>,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageNode {
    #[serde(flatten)]
    pub page: PageSummary,
    pub children: Vec<PageNode>,
}

fn must_update(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
    what: &str,
) -> VaultResult<()> {
    let n = conn.execute(sql, params)?;
    if n == 0 {
        return Err(VaultError::NotFound(what.to_string()));
    }
    Ok(())
}

fn row_notebook(r: &rusqlite::Row<'_>) -> rusqlite::Result<Notebook> {
    Ok(Notebook {
        id: r.get(0)?,
        name: r.get(1)?,
        color: r.get(2)?,
        sort_order: r.get(3)?,
    })
}

fn row_group(r: &rusqlite::Row<'_>) -> rusqlite::Result<SectionGroup> {
    Ok(SectionGroup {
        id: r.get(0)?,
        notebook_id: r.get(1)?,
        parent_group_id: r.get(2)?,
        name: r.get(3)?,
        sort_order: r.get(4)?,
    })
}

fn row_section(r: &rusqlite::Row<'_>) -> rusqlite::Result<Section> {
    Ok(Section {
        id: r.get(0)?,
        notebook_id: r.get(1)?,
        section_group_id: r.get(2)?,
        name: r.get(3)?,
        color: r.get(4)?,
        sort_order: r.get(5)?,
        is_encrypted: r.get::<_, i64>(6)? != 0,
        is_unlocked: false, // command 层结合 SessionManager 填充
    })
}

fn row_page_summary(r: &rusqlite::Row<'_>) -> rusqlite::Result<PageSummary> {
    Ok(PageSummary {
        id: r.get(0)?,
        section_id: r.get(1)?,
        parent_page_id: r.get(2)?,
        title: r.get(3)?,
        sort_order: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

// ---------------------------------------------------------------- 笔记本

pub fn create_notebook(
    conn: &Connection,
    name: &str,
    color: Option<&str>,
) -> VaultResult<Notebook> {
    let nb = Notebook {
        id: new_id(),
        name: name.to_string(),
        color: color.map(str::to_string),
        sort_order: 0,
    };
    conn.execute(
        "INSERT INTO notebooks (id, name, color, sort_order) VALUES (?1, ?2, ?3, ?4)",
        params![nb.id, nb.name, nb.color, nb.sort_order],
    )?;
    Ok(nb)
}

pub fn rename_notebook(conn: &Connection, id: &str, name: &str) -> VaultResult<()> {
    must_update(
        conn,
        "UPDATE notebooks SET name = ?1, updated_at = datetime('now') WHERE id = ?2",
        &params![name, id],
        &format!("笔记本 {id}"),
    )
}

pub fn set_notebook_color(conn: &Connection, id: &str, color: Option<&str>) -> VaultResult<()> {
    must_update(
        conn,
        "UPDATE notebooks SET color = ?1, updated_at = datetime('now') WHERE id = ?2",
        &params![color, id],
        &format!("笔记本 {id}"),
    )
}

/// 拖拽排序：按传入顺序重写 sort_order。
pub fn reorder_notebooks(conn: &Connection, ids: &[String]) -> VaultResult<()> {
    for (i, id) in ids.iter().enumerate() {
        must_update(
            conn,
            "UPDATE notebooks SET sort_order = ?1 WHERE id = ?2",
            &params![i as i64, id],
            &format!("笔记本 {id}"),
        )?;
    }
    Ok(())
}

/// 收集笔记本下全部分区 id（含嵌套分区组）。
fn section_ids_of_notebook(conn: &Connection, notebook_id: &str) -> VaultResult<Vec<String>> {
    let mut ids = Vec::new();
    let mut stmt = conn.prepare("SELECT id FROM sections WHERE notebook_id = ?1")?;
    let direct = stmt
        .query_map(params![notebook_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ids.extend(direct);
    Ok(ids)
}

/// 回收指定分区的全部页面（软删除进回收站）。返回受影响页面数。
fn recycle_pages_of_sections(conn: &Connection, section_ids: &[String]) -> VaultResult<usize> {
    if section_ids.is_empty() {
        return Ok(0);
    }
    let placeholders = section_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "UPDATE pages SET is_deleted = 1 WHERE is_deleted = 0 AND section_id IN ({placeholders})"
    );
    let refs: Vec<&dyn rusqlite::ToSql> = section_ids
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    Ok(conn.execute(&sql, refs.as_slice())?)
}

/// 删除笔记本：页面进回收站，分区/分区组/笔记本行删除。
///
/// 注意：回收站页面仍保留 `section_id` 引用（恢复需要），因此删除容器行时
/// 须在事务外临时关闭外键（SQLite 不允许事务内切换），提交后立即恢复。
fn delete_container_rows(
    conn: &mut Connection,
    delete_sql: &[&str],
    anchor: (&str, &str),
) -> VaultResult<()> {
    conn.pragma_update(None, "foreign_keys", false)?;
    let result = (|| -> VaultResult<()> {
        let tx = conn.transaction()?;
        for sql in delete_sql {
            tx.execute(sql, params![anchor.1])?;
        }
        let n = tx.execute(anchor.0, params![anchor.1])?;
        if n == 0 {
            return Err(VaultError::NotFound(format!("容器 {}", anchor.1)));
        }
        tx.commit()?;
        Ok(())
    })();
    conn.pragma_update(None, "foreign_keys", true)?;
    result
}

/// 删除笔记本：页面进回收站，分区/分区组/笔记本行删除。
pub fn delete_notebook(conn: &mut Connection, id: &str) -> VaultResult<()> {
    let sections = section_ids_of_notebook(conn, id)?;
    recycle_pages_of_sections(conn, &sections)?;
    delete_container_rows(
        conn,
        &[
            "DELETE FROM sections WHERE notebook_id = ?1",
            "DELETE FROM section_groups WHERE notebook_id = ?1",
        ],
        ("DELETE FROM notebooks WHERE id = ?1", id),
    )
}

// ---------------------------------------------------------------- 分区组

pub fn create_section_group(
    conn: &Connection,
    notebook_id: &str,
    parent_group_id: Option<&str>,
    name: &str,
) -> VaultResult<SectionGroup> {
    if let Some(parent) = parent_group_id {
        let parent_nb: String = conn
            .query_row(
                "SELECT notebook_id FROM section_groups WHERE id = ?1",
                params![parent],
                |r| r.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("分区组 {parent}")))?;
        if parent_nb != notebook_id {
            return Err(VaultError::Validation("父分区组不属于该笔记本".into()));
        }
    }
    let group = SectionGroup {
        id: new_id(),
        notebook_id: notebook_id.to_string(),
        parent_group_id: parent_group_id.map(str::to_string),
        name: name.to_string(),
        sort_order: 0,
    };
    conn.execute(
        "INSERT INTO section_groups (id, notebook_id, parent_group_id, name, sort_order) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![group.id, group.notebook_id, group.parent_group_id, group.name, group.sort_order],
    )?;
    Ok(group)
}

pub fn rename_section_group(conn: &Connection, id: &str, name: &str) -> VaultResult<()> {
    must_update(
        conn,
        "UPDATE section_groups SET name = ?1 WHERE id = ?2",
        &params![name, id],
        &format!("分区组 {id}"),
    )
}

/// 收集分区组及其全部子孙组 id。
fn descendant_group_ids(conn: &Connection, group_id: &str) -> VaultResult<Vec<String>> {
    let mut out = vec![group_id.to_string()];
    let mut frontier = vec![group_id.to_string()];
    while let Some(current) = frontier.pop() {
        let mut stmt = conn.prepare("SELECT id FROM section_groups WHERE parent_group_id = ?1")?;
        let children = stmt
            .query_map(params![current], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for child in children {
            out.push(child.clone());
            frontier.push(child);
        }
    }
    Ok(out)
}

/// 移动分区组（含跨笔记本）：组本身换父/换笔记本，其下分区与子孙组的
/// notebook_id 级联更新（schema 中分区冗余存 notebook_id 以加速导航）。
pub fn move_section_group(
    conn: &mut Connection,
    id: &str,
    notebook_id: &str,
    parent_group_id: Option<&str>,
) -> VaultResult<()> {
    if let Some(parent) = parent_group_id {
        if parent == id || descendant_group_ids(conn, id)?.contains(&parent.to_string()) {
            return Err(VaultError::Validation(
                "不能把分区组移动到自己或其子孙组下".into(),
            ));
        }
    }
    let tx = conn.transaction()?;
    must_update(
        &tx,
        "UPDATE section_groups SET notebook_id = ?1, parent_group_id = ?2 WHERE id = ?3",
        &params![notebook_id, parent_group_id, id],
        &format!("分区组 {id}"),
    )?;
    let group_ids = descendant_group_ids(&tx, id)?;
    let placeholders = group_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql =
        format!("UPDATE sections SET notebook_id = ?1 WHERE section_group_id IN ({placeholders})");
    let mut refs: Vec<&dyn rusqlite::ToSql> = vec![&notebook_id as &dyn rusqlite::ToSql];
    refs.extend(group_ids.iter().map(|s| s as &dyn rusqlite::ToSql));
    tx.execute(&sql, refs.as_slice())?;
    tx.commit()?;
    Ok(())
}

/// 删除分区组：组内（含嵌套组）分区的页面进回收站，组与分区行删除。
/// 容器行删除阶段同样临时关闭外键（回收站页面保留 section_id 引用）。
pub fn delete_section_group(conn: &mut Connection, id: &str) -> VaultResult<()> {
    let group_ids = descendant_group_ids(conn, id)?;
    let placeholders = group_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let find_sql = format!("SELECT id FROM sections WHERE section_group_id IN ({placeholders})");
    let refs: Vec<&dyn rusqlite::ToSql> = group_ids
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    let sections = {
        let mut stmt = conn.prepare(&find_sql)?;
        let rows = stmt
            .query_map(refs.as_slice(), |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    recycle_pages_of_sections(conn, &sections)?;
    conn.pragma_update(None, "foreign_keys", false)?;
    let result = (|| -> VaultResult<()> {
        let tx = conn.transaction()?;
        tx.execute(
            &format!("DELETE FROM sections WHERE section_group_id IN ({placeholders})"),
            refs.as_slice(),
        )?;
        tx.execute(
            &format!("DELETE FROM section_groups WHERE id IN ({placeholders})"),
            refs.as_slice(),
        )?;
        tx.commit()?;
        Ok(())
    })();
    conn.pragma_update(None, "foreign_keys", true)?;
    result
}

// ---------------------------------------------------------------- 分区

pub fn create_section(
    conn: &Connection,
    notebook_id: &str,
    section_group_id: Option<&str>,
    name: &str,
    color: Option<&str>,
) -> VaultResult<Section> {
    let section = Section {
        id: new_id(),
        notebook_id: notebook_id.to_string(),
        section_group_id: section_group_id.map(str::to_string),
        name: name.to_string(),
        color: color.map(str::to_string),
        sort_order: 0,
        is_encrypted: false,
        is_unlocked: false,
    };
    conn.execute(
        "INSERT INTO sections (id, notebook_id, section_group_id, name, color, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![section.id, section.notebook_id, section.section_group_id, section.name, section.color, section.sort_order],
    )?;
    Ok(section)
}

pub fn rename_section(conn: &Connection, id: &str, name: &str) -> VaultResult<()> {
    must_update(
        conn,
        "UPDATE sections SET name = ?1 WHERE id = ?2",
        &params![name, id],
        &format!("分区 {id}"),
    )
}

pub fn set_section_color(conn: &Connection, id: &str, color: Option<&str>) -> VaultResult<()> {
    must_update(
        conn,
        "UPDATE sections SET color = ?1 WHERE id = ?2",
        &params![color, id],
        &format!("分区 {id}"),
    )
}

/// 移动分区（跨分区组/笔记本）。
pub fn move_section(
    conn: &Connection,
    id: &str,
    notebook_id: &str,
    section_group_id: Option<&str>,
) -> VaultResult<()> {
    must_update(
        conn,
        "UPDATE sections SET notebook_id = ?1, section_group_id = ?2 WHERE id = ?3",
        &params![notebook_id, section_group_id, id],
        &format!("分区 {id}"),
    )
}

pub fn reorder_sections(conn: &Connection, ids: &[String]) -> VaultResult<()> {
    for (i, id) in ids.iter().enumerate() {
        must_update(
            conn,
            "UPDATE sections SET sort_order = ?1 WHERE id = ?2",
            &params![i as i64, id],
            &format!("分区 {id}"),
        )?;
    }
    Ok(())
}

/// 删除分区：页面进回收站，分区行删除（回收站页面保留 section_id 引用，
/// 删除容器行时临时关闭外键，提交后恢复）。
pub fn delete_section(conn: &mut Connection, id: &str) -> VaultResult<()> {
    recycle_pages_of_sections(conn, &[id.to_string()])?;
    delete_container_rows(conn, &[], ("DELETE FROM sections WHERE id = ?1", id))
}

// ---------------------------------------------------------------- 页面

pub fn create_page(
    conn: &Connection,
    section_id: &str,
    parent_page_id: Option<&str>,
    title: &str,
) -> VaultResult<PageSummary> {
    create_page_prepared(conn,section_id,parent_page_id,title,false)
}

pub(crate) fn create_page_prepared(conn:&Connection,section_id:&str,parent_page_id:Option<&str>,title:&str,title_is_encrypted:bool)->VaultResult<PageSummary> {
    let page = PageSummary {
        id: new_id(),
        section_id: section_id.to_string(),
        parent_page_id: parent_page_id.map(str::to_string),
        title: title.to_string(),
        sort_order: 0,
        updated_at: String::new(),
    };
    conn.execute(
        "INSERT INTO pages (id, section_id, parent_page_id, title, sort_order, title_is_encrypted) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![page.id, page.section_id, page.parent_page_id, page.title, page.sort_order,title_is_encrypted],
    )?;
    Ok(page)
}

pub fn rename_page(conn: &Connection, id: &str, title: &str) -> VaultResult<()> {
    must_update(
        conn,
        "UPDATE pages SET title = ?1 WHERE id = ?2 AND is_deleted = 0",
        &params![title, id],
        &format!("页面 {id}"),
    )
}

/// 移动/排序页面：可换分区、换父页面（多级层级）与排序值。
pub fn move_page(
    conn: &Connection,
    id: &str,
    section_id: &str,
    parent_page_id: Option<&str>,
    sort_order: i64,
) -> VaultResult<()> {
    if let Some(parent) = parent_page_id {
        if parent == id {
            return Err(VaultError::Validation("页面不能移动到自己之下".into()));
        }
    }
    must_update(
        conn,
        "UPDATE pages SET section_id = ?1, parent_page_id = ?2, sort_order = ?3 WHERE id = ?4 AND is_deleted = 0",
        &params![section_id, parent_page_id, sort_order, id],
        &format!("页面 {id}"),
    )
}

/// 删除页面 → 回收站（软删除）。
pub fn delete_page(conn: &Connection, id: &str) -> VaultResult<()> {
    must_update(
        conn,
        "UPDATE pages SET is_deleted = 1 WHERE id = ?1 AND is_deleted = 0",
        &params![id],
        &format!("页面 {id}"),
    )
}

/// 从回收站恢复（原分区必须仍存在）。
pub fn restore_page(conn: &Connection, id: &str) -> VaultResult<()> {
    let section_id: Option<String> = conn
        .query_row(
            "SELECT section_id FROM pages WHERE id = ?1 AND is_deleted = 1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    let section_id = section_id.ok_or_else(|| VaultError::NotFound(format!("回收站页面 {id}")))?;
    let section_exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sections WHERE id = ?1",
        params![section_id],
        |r| r.get(0),
    )?;
    if section_exists == 0 {
        return Err(VaultError::NotFound(format!(
            "页面原分区已被删除，无法恢复到原分区 ({section_id})"
        )));
    }
    conn.execute("UPDATE pages SET is_deleted = 0 WHERE id = ?1", params![id])?;
    Ok(())
}

/// 彻底删除：物理移除页面、其版本与附件登记（附件文件清理见 attachments 层）。
pub fn purge_page(conn: &Connection, id: &str) -> VaultResult<()> {
    conn.execute(
        "DELETE FROM attachments WHERE entity_type = 'page' AND entity_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM page_versions WHERE page_id = ?1", params![id])?;
    let n = conn.execute(
        "DELETE FROM pages WHERE id = ?1 AND is_deleted = 1",
        params![id],
    )?;
    if n == 0 {
        return Err(VaultError::NotFound(format!("回收站页面 {id}")));
    }
    Ok(())
}

/// 回收站列表（附原分区/笔记本名供展示）。
pub fn list_trash(conn: &Connection) -> VaultResult<Vec<serde_json::Value>> {
    // 容器已删的回收站页面仍要可见（LEFT JOIN，名称兜底）
    let mut stmt = conn.prepare(
        "SELECT p.id, p.title, p.updated_at,
                COALESCE(s.name, '(原分区已删除)'), COALESCE(n.name, '(原笔记本已删除)')
         FROM pages p
         LEFT JOIN sections s ON s.id = p.section_id
         LEFT JOIN notebooks n ON n.id = s.notebook_id
         WHERE p.is_deleted = 1
         ORDER BY p.updated_at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "title": r.get::<_, String>(1)?,
                "deleted_at": r.get::<_, String>(2)?,
                "section_name": r.get::<_, String>(3)?,
                "notebook_name": r.get::<_, String>(4)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------- 导航树

fn load_groups(conn: &Connection, notebook_id: &str) -> VaultResult<Vec<SectionGroup>> {
    let mut stmt = conn.prepare(
        "SELECT id, notebook_id, parent_group_id, name, sort_order FROM section_groups WHERE notebook_id = ?1 ORDER BY sort_order, name",
    )?;
    let rows = stmt
        .query_map(params![notebook_id], row_group)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn load_sections(
    conn: &Connection,
    notebook_id: &str,
    group_id: Option<&str>,
) -> VaultResult<Vec<Section>> {
    let (sql, param): (&str, Option<&str>) = match group_id {
        Some(g) => (
            "SELECT id, notebook_id, section_group_id, name, color, sort_order, is_encrypted
             FROM sections WHERE notebook_id = ?1 AND section_group_id = ?2 ORDER BY sort_order, name",
            Some(g),
        ),
        None => (
            "SELECT id, notebook_id, section_group_id, name, color, sort_order, is_encrypted
             FROM sections WHERE notebook_id = ?1 AND section_group_id IS NULL ORDER BY sort_order, name",
            None,
        ),
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = match param {
        Some(g) => stmt
            .query_map(params![notebook_id, g], row_section)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        None => stmt
            .query_map(params![notebook_id], row_section)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };
    Ok(rows)
}

fn build_group_node(
    conn: &Connection,
    group: &SectionGroup,
    all: &[SectionGroup],
) -> VaultResult<SectionGroupNode> {
    let children = all
        .iter()
        .filter(|g| g.parent_group_id.as_deref() == Some(&group.id))
        .cloned()
        .collect::<Vec<_>>();
    let mut child_nodes = Vec::new();
    for child in &children {
        child_nodes.push(build_group_node(conn, child, all)?);
    }
    Ok(SectionGroupNode {
        group: group.clone(),
        children: child_nodes,
        sections: load_sections(conn, &group.notebook_id, Some(&group.id))?,
    })
}

/// 笔记本导航树（分区组任意嵌套）。
pub fn notebook_tree(conn: &Connection) -> VaultResult<Vec<NotebookNode>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, color, sort_order FROM notebooks WHERE id != '__page_storage__' ORDER BY sort_order, created_at",
    )?;
    let notebooks = stmt
        .query_map([], row_notebook)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut nodes = Vec::new();
    for nb in notebooks {
        let all_groups = load_groups(conn, &nb.id)?;
        let roots: Vec<&SectionGroup> = all_groups
            .iter()
            .filter(|g| g.parent_group_id.is_none())
            .collect();
        let mut group_nodes = Vec::new();
        for root in roots {
            group_nodes.push(build_group_node(conn, root, &all_groups)?);
        }
        let sections = load_sections(conn, &nb.id, None)?;
        nodes.push(NotebookNode {
            notebook: nb,
            groups: group_nodes,
            sections,
        });
    }
    Ok(nodes)
}

/// 分区内的页面树（多级子页面）。
pub fn page_tree(conn: &Connection, section_id: &str) -> VaultResult<Vec<PageNode>> {
    let mut stmt = conn.prepare(
        "SELECT id, section_id, parent_page_id, title, sort_order, updated_at
         FROM pages WHERE section_id = ?1 AND is_deleted = 0 ORDER BY sort_order, title",
    )?;
    let pages = stmt
        .query_map(params![section_id], row_page_summary)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(build_page_nodes(&pages, None))
}

fn build_page_nodes(pages: &[PageSummary], parent: Option<&str>) -> Vec<PageNode> {
    pages
        .iter()
        .filter(|p| p.parent_page_id.as_deref() == parent)
        .map(|p| PageNode {
            page: p.clone(),
            children: build_page_nodes(pages, Some(&p.id)),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate::{run_migrations, DbKind};

    fn space_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        conn
    }

    #[test]
    fn notebook_crud_and_color() {
        let conn = space_db();
        let nb = create_notebook(&conn, "工作", Some("#ff0000")).unwrap();
        rename_notebook(&conn, &nb.id, "工作笔记").unwrap();
        set_notebook_color(&conn, &nb.id, None).unwrap();
        let tree = notebook_tree(&conn).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].notebook.name, "工作笔记");
        assert_eq!(tree[0].notebook.color, None);
    }

    #[test]
    fn reorder_persists_sort_order() {
        let conn = space_db();
        let a = create_notebook(&conn, "A", None).unwrap();
        let b = create_notebook(&conn, "B", None).unwrap();
        reorder_notebooks(&conn, &[b.id.clone(), a.id.clone()]).unwrap();
        let tree = notebook_tree(&conn).unwrap();
        assert_eq!(tree[0].notebook.id, b.id);
        assert_eq!(tree[1].notebook.id, a.id);
    }

    #[test]
    fn nested_groups_render_tree() {
        let conn = space_db();
        let nb = create_notebook(&conn, "nb", None).unwrap();
        let g1 = create_section_group(&conn, &nb.id, None, "组一").unwrap();
        let g2 = create_section_group(&conn, &nb.id, Some(&g1.id), "组二").unwrap();
        create_section(&conn, &nb.id, Some(&g2.id), "加密候选", None).unwrap();
        let tree = notebook_tree(&conn).unwrap();
        assert_eq!(tree[0].groups.len(), 1);
        assert_eq!(tree[0].groups[0].children.len(), 1);
        assert_eq!(tree[0].groups[0].children[0].sections.len(), 1);
        assert_eq!(tree[0].groups[0].children[0].sections[0].name, "加密候选");
    }

    #[test]
    fn group_cannot_move_into_own_descendant() {
        let mut conn = space_db();
        let nb = create_notebook(&conn, "nb", None).unwrap();
        let g1 = create_section_group(&conn, &nb.id, None, "g1").unwrap();
        let g2 = create_section_group(&conn, &nb.id, Some(&g1.id), "g2").unwrap();
        assert!(matches!(
            move_section_group(&mut conn, &g1.id, &nb.id, Some(&g2.id)),
            Err(VaultError::Validation(_))
        ));
    }

    #[test]
    fn cross_notebook_group_move_cascades_section_notebook_id() {
        let mut conn = space_db();
        let nb1 = create_notebook(&conn, "nb1", None).unwrap();
        let nb2 = create_notebook(&conn, "nb2", None).unwrap();
        let g = create_section_group(&conn, &nb1.id, None, "g").unwrap();
        let sec = create_section(&conn, &nb1.id, Some(&g.id), "s", None).unwrap();
        move_section_group(&mut conn, &g.id, &nb2.id, None).unwrap();
        let tree = notebook_tree(&conn).unwrap();
        let nb2_node = tree.iter().find(|n| n.notebook.id == nb2.id).unwrap();
        assert_eq!(nb2_node.groups[0].sections[0].id, sec.id);
        let section_nb: String = conn
            .query_row(
                "SELECT notebook_id FROM sections WHERE id = ?1",
                params![sec.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(section_nb, nb2.id, "分区 notebook_id 必须级联更新");
    }

    #[test]
    fn delete_notebook_recycles_pages_not_physical() {
        let mut conn = space_db();
        let nb = create_notebook(&conn, "nb", None).unwrap();
        let sec = create_section(&conn, &nb.id, None, "s", None).unwrap();
        let page = create_page(&conn, &sec.id, None, "p").unwrap();
        delete_notebook(&mut conn, &nb.id).unwrap();
        let trash = list_trash(&conn).unwrap();
        assert_eq!(trash.len(), 1);
        assert_eq!(trash[0]["id"], page.id);
        // 物理数据仍在
        let content: String = conn
            .query_row(
                "SELECT content FROM pages WHERE id = ?1",
                params![page.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(content, "");
    }

    #[test]
    fn page_tree_multi_level_and_move() {
        let conn = space_db();
        let nb = create_notebook(&conn, "nb", None).unwrap();
        let sec = create_section(&conn, &nb.id, None, "s", None).unwrap();
        let p1 = create_page(&conn, &sec.id, None, "父").unwrap();
        let p2 = create_page(&conn, &sec.id, Some(&p1.id), "子").unwrap();
        let p3 = create_page(&conn, &sec.id, Some(&p2.id), "孙").unwrap();
        let tree = page_tree(&conn, &sec.id).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children[0].page.id, p2.id);
        assert_eq!(tree[0].children[0].children[0].page.id, p3.id);

        // 拖拽：子页面提升为顶级并排序
        move_page(&conn, &p3.id, &sec.id, None, 0).unwrap();
        let tree = page_tree(&conn, &sec.id).unwrap();
        assert_eq!(tree[0].page.id, p3.id);

        // 页面不能作为自己的父
        assert!(matches!(
            move_page(&conn, &p1.id, &sec.id, Some(&p1.id), 0),
            Err(VaultError::Validation(_))
        ));
    }

    #[test]
    fn delete_restore_purge_cycle() {
        let conn = space_db();
        let nb = create_notebook(&conn, "nb", None).unwrap();
        let sec = create_section(&conn, &nb.id, None, "s", None).unwrap();
        let page = create_page(&conn, &sec.id, None, "p").unwrap();
        delete_page(&conn, &page.id).unwrap();
        assert_eq!(list_trash(&conn).unwrap().len(), 1);
        // 正常恢复
        restore_page(&conn, &page.id).unwrap();
        assert!(list_trash(&conn).unwrap().is_empty());
        // 再次删除并彻底删除
        delete_page(&conn, &page.id).unwrap();
        purge_page(&conn, &page.id).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pages WHERE id = ?1",
                params![page.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn restore_fails_when_original_section_gone() {
        let mut conn = space_db();
        let nb = create_notebook(&conn, "nb", None).unwrap();
        let sec = create_section(&conn, &nb.id, None, "s", None).unwrap();
        let page = create_page(&conn, &sec.id, None, "p").unwrap();
        delete_page(&conn, &page.id).unwrap();
        delete_section(&mut conn, &sec.id).unwrap();
        assert!(matches!(
            restore_page(&conn, &page.id),
            Err(VaultError::NotFound(_))
        ));
        // 彻底删除仍可用
        purge_page(&conn, &page.id).unwrap();
    }

    #[test]
    fn cross_notebook_section_move() {
        let conn = space_db();
        let nb1 = create_notebook(&conn, "nb1", None).unwrap();
        let nb2 = create_notebook(&conn, "nb2", None).unwrap();
        let sec = create_section(&conn, &nb1.id, None, "s", Some("#00ff00")).unwrap();
        move_section(&conn, &sec.id, &nb2.id, None).unwrap();
        let tree = notebook_tree(&conn).unwrap();
        let nb2_node = tree.iter().find(|n| n.notebook.id == nb2.id).unwrap();
        assert_eq!(nb2_node.sections.len(), 1);
        assert_eq!(nb2_node.sections[0].color.as_deref(), Some("#00ff00"));
    }

    #[test]
    fn section_reorder_persists() {
        let conn = space_db();
        let nb = create_notebook(&conn, "nb", None).unwrap();
        let a = create_section(&conn, &nb.id, None, "a", None).unwrap();
        let b = create_section(&conn, &nb.id, None, "b", None).unwrap();
        reorder_sections(&conn, &[b.id.clone(), a.id.clone()]).unwrap();
        let tree = notebook_tree(&conn).unwrap();
        assert_eq!(tree[0].sections[0].id, b.id);
        assert_eq!(tree[0].sections[1].id, a.id);
    }
}
