//! FTS5 全文搜索（spec: notes-module §笔记全文搜索）。
//!
//! - 持久索引：空间库 `pages_fts`（迁移 0002_fts.sql，触发器维护；
//!   加密分区页面由触发器按 `sections.is_encrypted` 分派，完全不进入持久索引）。
//! - 解锁分区：内存临时 FTS 表（TEMP schema，随连接销毁；锁定即 DROP），
//!   查询时与持久表结果合并。
//! - rowid 约定：FTS 表 rowid == pages.rowid。
//! - 中文分词：FTS5 `unicode61` 不识别 CJK。采用应用层单字切分——自定义标量函数
//!   `tv_seg`（在 `db::connection::configure` 注册到每个连接）把 CJK 字符以空格
//!   分隔后写入索引；查询关键词经同一函数处理后 MATCH。符合设计风险节
//!   「M2 接受单字分词」的既定决策；jieba 分词留作 M4 优化项。
//!
//! 注意：临时表在 SQL 中必须以不带 schema 限定的裸表名引用（FTS5 snippet 等
//! 表名参数不接受 `temp.` 前缀），TEMP schema 的解析优先级保证命中临时表。

/// CJK 单字切分：东亚文字逐字以空格分隔，其余字符原样保留。
/// 供 FTS 触发器与查询构造共用（保证索引与查询的分词口径一致）。
pub fn segment_cjk(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut prev_cjk = false;
    for ch in text.chars() {
        let cjk = matches!(ch,
            '\u{1100}'..='\u{11FF}'   // Hangul Jamo
            | '\u{2E80}'..='\u{9FFF}' // CJK Radicals .. CJK Unified Ideographs Ext B
            | '\u{A000}'..='\u{A4CF}' // Yi
            | '\u{AC00}'..='\u{D7AF}' // Hangul Syllables
            | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
            | '\u{FF66}'..='\u{FF9F}' // Halfwidth Katakana
        );
        if cjk {
            if prev_cjk {
                out.push(' ');
            }
            out.push(ch);
        } else {
            out.push(ch);
        }
        prev_cjk = cjk;
    }
    out
}

/// 还原单字切分显示：去掉 CJK 字符之间的切分空格（用于 snippet 展示）。
fn desegment_cjk(text: &str) -> String {
    fn is_cjk_boundary(c: char) -> bool {
        matches!(c,
            '\u{1100}'..='\u{11FF}' | '\u{2E80}'..='\u{9FFF}' | '\u{A000}'..='\u{A4CF}'
            | '\u{AC00}'..='\u{D7AF}' | '\u{F900}'..='\u{FAFF}' | '\u{FF66}'..='\u{FF9F}')
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &ch) in chars.iter().enumerate() {
        if ch == ' '
            && i > 0
            && i + 1 < chars.len()
            && is_cjk_boundary(chars[i - 1])
            && is_cjk_boundary(chars[i + 1])
        {
            continue;
        }
        out.push(ch);
    }
    out
}

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::{VaultError, VaultResult};

/// 一条搜索结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub page_id: String,
    pub section_id: String,
    pub title: String,
    /// 带高亮标记的正文片段（`<mark>` 包裹命中词）
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskSearchHit {
    pub task_id: String,
    pub title: String,
    pub snippet: String,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventSearchHit {
    pub event_id: String,
    pub title: String,
    pub snippet: String,
}

/// 解锁分区内存临时 FTS 表名（仅允许字母数字下划线，防注入）。
fn temp_table_name(section_id: &str) -> String {
    let sanitized: String = section_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("pages_fts_unlocked_{sanitized}")
}

/// 为已解锁的加密分区在 TEMP schema 建立内存临时 FTS 表并灌入明文索引。
/// 重复调用先重建（DROP + CREATE），保证幂等。
pub fn build_unlocked_index(
    conn: &Connection,
    section_id: &str,
    pages: &[(i64, String, String)],
) -> VaultResult<()> {
    let table = temp_table_name(section_id);
    conn.execute_batch(&format!(
        "DROP TABLE IF EXISTS temp.{table};
         CREATE VIRTUAL TABLE temp.{table} USING fts5(title, content, tokenize = 'unicode61');"
    ))?;
    {
        let mut stmt = conn.prepare(&format!(
            "INSERT INTO temp.{table} (rowid, title, content) VALUES (?1, ?2, ?3)"
        ))?;
        for (rowid, title, content) in pages {
            // 与持久索引触发器同一分词口径（tv_seg）
            stmt.execute(params![rowid, segment_cjk(title), segment_cjk(content)])?;
        }
    }
    Ok(())
}

/// 销毁解锁分区的内存临时索引（锁定收口，绝不残留）。
pub fn drop_unlocked_index(conn: &Connection, section_id: &str) -> VaultResult<()> {
    let table = temp_table_name(section_id);
    conn.execute_batch(&format!("DROP TABLE IF EXISTS temp.{table}"))?;
    Ok(())
}

/// 临时索引是否仍存在（测试与断言用）。
pub fn temp_index_exists(conn: &Connection, section_id: &str) -> VaultResult<bool> {
    let table = temp_table_name(section_id);
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_temp_master WHERE type = 'table' AND name = ?1",
        params![table],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 用户输入 → FTS5 MATCH 查询串：先按 `segment_cjk` 统一分词口径，
/// 再按空白切词、逐词短语匹配（AND 语义），引号转义为 FTS5 双引号字面量。
fn to_fts_query(input: &str) -> VaultResult<String> {
    let segmented = segment_cjk(input);
    let tokens: Vec<&str> = segmented.split_whitespace().collect();
    if tokens.is_empty() {
        return Err(VaultError::Validation("搜索关键词为空".into()));
    }
    Ok(tokens
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" "))
}

/// 在临时表上搜索。`table` 必须是不带 schema 限定的裸表名
/// （FTS5 snippet/MATCH 的表名参数不接受 `temp.` 前缀）。
fn search_one_table(
    conn: &Connection,
    table: &str,
    fts_query: &str,
    limit: usize,
) -> VaultResult<Vec<SearchHit>> {
    let sql = format!(
        "SELECT p.id, p.section_id, p.title,
                snippet({table}, 1, '<mark>', '</mark>', '…', 16) AS snippet
         FROM {table} JOIN pages p ON p.rowid = {table}.rowid
         WHERE {table} MATCH ?1
         ORDER BY rank
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let hits = stmt
        .query_map(params![fts_query, limit as i64], |r| {
            Ok(SearchHit {
                page_id: r.get(0)?,
                section_id: r.get(1)?,
                title: r.get(2)?,
                snippet: desegment_cjk(&r.get::<_, String>(3)?),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(hits)
}

/// 全局搜索：持久 `pages_fts` + 各解锁分区的内存临时表，结果合并截断。
pub fn search(
    conn: &Connection,
    query: &str,
    unlocked_section_ids: &[String],
    limit: usize,
) -> VaultResult<Vec<SearchHit>> {
    let fts_query = to_fts_query(query)?;
    let mut hits = search_one_table(conn, "pages_fts", &fts_query, limit)?;
    for section_id in unlocked_section_ids {
        if !temp_index_exists(conn, section_id)? {
            continue;
        }
        // 裸表名：TEMP schema 解析优先，FTS5 表名参数不接受 schema 前缀
        let table = temp_table_name(section_id);
        let mut extra = search_one_table(conn, &table, &fts_query, limit)?;
        hits.append(&mut extra);
    }
    hits.truncate(limit);
    Ok(hits)
}

pub fn search_tasks(
    conn: &Connection,
    query: &str,
    include_archived: bool,
    limit: usize,
) -> VaultResult<Vec<TaskSearchHit>> {
    let fts_query = to_fts_query(query)?;
    let archive_filter = if include_archived {
        ""
    } else {
        "AND t.archived_at IS NULL"
    };
    let sql = format!(
        "SELECT t.id, t.title,
                snippet(tasks_fts, 1, '<mark>', '</mark>', '…', 16),
                t.archived_at IS NOT NULL
         FROM tasks_fts
         JOIN tasks t ON t.rowid = tasks_fts.rowid
         WHERE tasks_fts MATCH ?1 {archive_filter}
         ORDER BY rank
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let hits = stmt
        .query_map(params![fts_query, limit as i64], |row| {
            Ok(TaskSearchHit {
                task_id: row.get(0)?,
                title: row.get(1)?,
                snippet: desegment_cjk(&row.get::<_, String>(2)?),
                archived: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(hits)
}

/// Search event bodies, not expanded occurrences. A recurring event therefore
/// appears exactly once and callers can navigate to its series definition.
pub fn search_events(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> VaultResult<Vec<EventSearchHit>> {
    let fts_query = to_fts_query(query)?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT e.id, e.title,
                snippet(events_fts, 1, '<mark>', '</mark>', '…', 16)
         FROM events_fts
         JOIN events e ON e.rowid = events_fts.rowid
         WHERE events_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;
    let hits = stmt
        .query_map(params![fts_query, limit as i64], |row| {
            Ok(EventSearchHit {
                event_id: row.get(0)?,
                title: row.get(1)?,
                snippet: desegment_cjk(&row.get::<_, String>(2)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::super::db::migrate::{run_migrations, DbKind};
    use super::*;

    /// 临时空间库：应用全部空间迁移（含 0002 FTS）。
    fn space_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        conn
    }

    fn add_hierarchy(conn: &Connection, encrypted: bool) -> (String, String) {
        conn.execute(
            "INSERT INTO notebooks (id, name) VALUES ('nb1', '笔记本')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sections (id, notebook_id, name, is_encrypted) VALUES ('sec1', 'nb1', '分区', ?1)",
            params![encrypted as i64],
        )
        .unwrap();
        ("nb1".into(), "sec1".into())
    }

    fn add_page(conn: &Connection, id: &str, title: &str, content: &str) {
        conn.execute(
            "INSERT INTO pages (id, section_id, title, content) VALUES (?1, 'sec1', ?2, ?3)",
            params![id, title, content],
        )
        .unwrap();
    }

    fn hits_for(conn: &Connection, query: &str) -> Vec<SearchHit> {
        search(conn, query, &[], 50).unwrap()
    }

    // ---- 1.1 迁移：索引与页面增删改同步 ----

    #[test]
    fn fts_syncs_on_insert_update_delete() {
        let conn = space_db();
        add_hierarchy(&conn, false);
        add_page(&conn, "p1", "部署清单", "服务器账号信息");

        // 标题命中
        let hits = hits_for(&conn, "部署");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page_id, "p1");
        // 正文命中且高亮标记落在正文片段上
        let hits = hits_for(&conn, "账号");
        assert_eq!(hits.len(), 1);
        assert!(
            hits[0].snippet.contains("<mark>"),
            "snippet 应高亮命中词: {}",
            hits[0].snippet
        );

        // 更新标题后按新标题可搜、按旧标题不可搜
        conn.execute("UPDATE pages SET title = '发布清单' WHERE id = 'p1'", [])
            .unwrap();
        assert_eq!(hits_for(&conn, "发布").len(), 1);
        assert!(hits_for(&conn, "部署").is_empty());

        // 更新正文后按新正文可搜
        conn.execute(
            "UPDATE pages SET content = '部署上下文记录' WHERE id = 'p1'",
            [],
        )
        .unwrap();
        assert_eq!(hits_for(&conn, "上下文").len(), 1);

        // 删除后从索引消失
        conn.execute("DELETE FROM pages WHERE id = 'p1'", [])
            .unwrap();
        assert!(hits_for(&conn, "发布").is_empty());
    }

    #[test]
    fn fts_backfills_existing_plain_pages() {
        // 模拟 0002 迁移前的库：仅 v1 schema，已含存量页面
        let conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        conn.execute_batch(include_str!("../../migrations/space/0001_init.sql"))
            .unwrap();
        add_hierarchy(&conn, false);
        add_page(&conn, "p1", "存量页面", "迁移前写入的内容");
        // 应用 0002 迁移文件本体（回填逻辑）
        conn.execute_batch(include_str!("../../migrations/space/0002_fts.sql"))
            .unwrap();
        let hits = hits_for(&conn, "迁移前");
        assert_eq!(hits.len(), 1, "存量明文页面必须回填索引");
    }

    #[test]
    fn deleted_pages_are_not_searchable() {
        let conn = space_db();
        add_hierarchy(&conn, false);
        add_page(&conn, "p1", "待删页面", "回收站内容");
        conn.execute("UPDATE pages SET is_deleted = 1 WHERE id = 'p1'", [])
            .unwrap();
        assert!(hits_for(&conn, "回收站").is_empty(), "回收站页面不得被搜到");
        // 恢复后重新可搜
        conn.execute("UPDATE pages SET is_deleted = 0 WHERE id = 'p1'", [])
            .unwrap();
        assert_eq!(hits_for(&conn, "回收站").len(), 1);
    }

    // ---- 1.3 加密分区页面不进入持久索引 ----

    #[test]
    fn encrypted_pages_stay_out_of_persistent_index() {
        let conn = space_db();
        add_hierarchy(&conn, true);
        // 明文状态下写入（如设置密码前的旧数据）也不进索引
        add_page(&conn, "p1", "加密页面", "敏感内容明文态");
        assert!(
            hits_for(&conn, "敏感").is_empty(),
            "加密分区页面不得进入持久索引"
        );

        conn.execute(
            "UPDATE pages SET content = '更新后的明文' WHERE id = 'p1'",
            [],
        )
        .unwrap();
        assert!(
            hits_for(&conn, "更新后").is_empty(),
            "加密分区页面更新后仍不得进索引"
        );
    }

    // ---- 1.2 解锁-搜索-锁定-再搜索全序列 ----

    #[test]
    fn unlocked_index_lifecycle_search_then_lock() {
        let conn = space_db();
        add_hierarchy(&conn, true);
        add_page(&conn, "p1", "加密页面", "解锁后才可搜索的敏感内容");
        let rowid: i64 = conn
            .query_row("SELECT rowid FROM pages WHERE id = 'p1'", [], |r| r.get(0))
            .unwrap();

        // 锁定：持久索引无结果，临时表不存在
        assert!(hits_for(&conn, "敏感").is_empty());
        assert!(!temp_index_exists(&conn, "sec1").unwrap());

        // 解锁：建内存临时索引 → 可搜
        build_unlocked_index(
            &conn,
            "sec1",
            &[(rowid, "加密页面".into(), "解锁后才可搜索的敏感内容".into())],
        )
        .unwrap();
        assert!(temp_index_exists(&conn, "sec1").unwrap());
        let hits = search(&conn, "敏感", &["sec1".to_string()], 50).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page_id, "p1");

        // 锁定：DROP 临时表 → 再搜索无结果且表已销毁
        drop_unlocked_index(&conn, "sec1").unwrap();
        assert!(!temp_index_exists(&conn, "sec1").unwrap());
        assert!(search(&conn, "敏感", &["sec1".to_string()], 50)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rebuild_unlocked_index_is_idempotent() {
        let conn = space_db();
        add_hierarchy(&conn, true);
        add_page(&conn, "p1", "t", "敏感词甲");
        let rowid: i64 = conn
            .query_row("SELECT rowid FROM pages WHERE id = 'p1'", [], |r| r.get(0))
            .unwrap();
        build_unlocked_index(&conn, "sec1", &[(rowid, "t".into(), "敏感词甲".into())]).unwrap();
        build_unlocked_index(&conn, "sec1", &[(rowid, "t".into(), "敏感词乙".into())]).unwrap();
        let hits = search(&conn, "敏感词乙", &["sec1".to_string()], 50).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(search(&conn, "敏感词甲", &["sec1".to_string()], 50)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn empty_query_rejected() {
        let conn = space_db();
        assert!(matches!(
            search(&conn, "   ", &[], 10),
            Err(VaultError::Validation(_))
        ));
    }

    #[test]
    fn task_search_hits_chinese_notes_and_filters_archived() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Tasks.migrations()).unwrap();
        conn.execute(
            "INSERT INTO task_lists (id, name) VALUES ('l', '收件箱')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, list_id, title, notes) VALUES ('active', 'l', '采购', '购买咖啡豆')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, list_id, title, notes, archived_at) VALUES ('archived', 'l', '旧采购', '购买咖啡豆', datetime('now'))",
            [],
        )
        .unwrap();

        let active = search_tasks(&conn, "咖啡", false, 20).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].task_id, "active");
        assert!(active[0].snippet.contains("<mark>"));
        let all = search_tasks(&conn, "咖啡", true, 20).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|hit| hit.archived));
    }

    #[test]
    fn recurring_event_search_returns_series_once() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Calendar.migrations()).unwrap();
        conn.execute(
            "INSERT INTO events (id, title, description, start_at, end_at, recurrence_rule)
             VALUES ('event', '例会', '讨论发布计划', '2026-09-20T10:00:00', '2026-09-20T11:00:00', 'FREQ=WEEKLY')",
            [],
        )
        .unwrap();
        let hits = search_events(&conn, "发布", 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].event_id, "event");
        assert!(hits[0].snippet.contains("<mark>"));
    }
}
