//! 嵌入式版本化迁移 runner。
//!
//! 迁移文件位于 `src-tauri/migrations/<db>/NNNN_*.sql`，编译期嵌入二进制。
//! 启动时比较 `schema_migrations` 表，单事务执行缺失迁移；已应用的迁移绝不重复执行。

use rusqlite::Connection;

use crate::error::{VaultError, VaultResult};

/// 一条迁移。
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: u64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// 数据库种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbKind {
    Meta,
    Tasks,
    Calendar,
    Space,
}

impl DbKind {
    pub fn migrations(&self) -> &'static [Migration] {
        match self {
            DbKind::Meta => &[
                Migration {
                    version: 1,
                    name: "init",
                    sql: include_str!("../../migrations/meta/0001_init.sql"),
                },
                Migration {
                    version: 2,
                    name: "recent_pages",
                    sql: include_str!("../../migrations/meta/0002_recent_pages.sql"),
                },
                Migration {
                    version: 3,
                    name: "reminder_fires",
                    sql: include_str!("../../migrations/meta/0003_reminder_fires.sql"),
                },
                Migration {
                    version: 4,
                    name: "templates_preferences",
                    sql: include_str!("../../migrations/meta/0004_templates_preferences.sql"),
                },
            ],
            DbKind::Tasks => &[
                Migration {
                    version: 1,
                    name: "init",
                    sql: include_str!("../../migrations/tasks/0001_init.sql"),
                },
                Migration {
                    version: 2,
                    name: "fts_archive",
                    sql: include_str!("../../migrations/tasks/0002_fts_archive.sql"),
                },
                Migration {
                    version: 3,
                    name: "note_links",
                    sql: include_str!("../../migrations/tasks/0003_note_links.sql"),
                },
                Migration {
                    version: 4,
                    name: "note_sync_trigger",
                    sql: include_str!("../../migrations/tasks/0004_note_sync_trigger.sql"),
                },
            ],
            DbKind::Calendar => &[
                Migration {
                    version: 1,
                    name: "init",
                    sql: include_str!("../../migrations/calendar/0001_init.sql"),
                },
                Migration {
                    version: 2,
                    name: "reminders_fts",
                    sql: include_str!("../../migrations/calendar/0002_reminders_fts.sql"),
                },
                Migration {
                    version: 3,
                    name: "external_uid_taggings",
                    sql: include_str!("../../migrations/calendar/0003_external_uid_taggings.sql"),
                },
            ],
            DbKind::Space => &[
                Migration {
                    version: 1,
                    name: "init",
                    sql: include_str!("../../migrations/space/0001_init.sql"),
                },
                Migration {
                    version: 2,
                    name: "fts",
                    sql: include_str!("../../migrations/space/0002_fts.sql"),
                },
                Migration {
                    version: 3,
                    name: "section_templates",
                    sql: include_str!("../../migrations/space/0003_section_templates.sql"),
                },
                Migration {
                    version: 4,
                    name: "page_tree",
                    sql: include_str!("../../migrations/space/0004_page_tree.sql"),
                },
            ],
        }
    }
}

fn validate(migrations: &[Migration]) -> VaultResult<()> {
    for w in migrations.windows(2) {
        if w[1].version <= w[0].version {
            return Err(VaultError::Migration(format!(
                "迁移版本必须严格递增: {} -> {}",
                w[0].version, w[1].version
            )));
        }
    }
    Ok(())
}

/// 将连接升级到最新版本。幂等：重复调用不做任何事。
pub fn run_migrations(conn: &mut Connection, migrations: &[Migration]) -> VaultResult<()> {
    validate(migrations)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;
    let applied: Vec<u64> = {
        let mut stmt = conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let versions = stmt
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<u64>, _>>()?;
        versions
    };
    let known: Vec<u64> = migrations.iter().map(|m| m.version).collect();
    for v in &applied {
        if !known.contains(v) {
            return Err(VaultError::Migration(format!(
                "数据库含有未知迁移版本 {v}，高于应用内置迁移，拒绝启动"
            )));
        }
    }
    for migration in migrations {
        if applied.contains(&migration.version) {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql).map_err(|e| {
            VaultError::Migration(format!(
                "迁移 {} ({}) 执行失败: {e}",
                migration.version, migration.name
            ))
        })?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
            rusqlite::params![migration.version as i64, migration.name],
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// 当前已应用的最新版本（空库为 0）。
pub fn current_version(conn: &Connection) -> VaultResult<u64> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
        [],
        |r| r.get::<_, i64>(0),
    ).map(|c| c > 0)?;
    if !exists {
        return Ok(0);
    }
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get::<_, i64>(0),
    )? as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Migration> {
        vec![
            Migration {
                version: 1,
                name: "one",
                sql: "CREATE TABLE t1 (id INTEGER PRIMARY KEY);",
            },
            Migration {
                version: 2,
                name: "two",
                sql: "CREATE TABLE t2 (id INTEGER PRIMARY KEY);",
            },
        ]
    }

    #[test]
    fn fresh_db_applies_all_migrations() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn, &sample()).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('t1', 't2')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn rerun_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn, &sample()).unwrap();
        run_migrations(&mut conn, &sample()).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);
    }

    #[test]
    fn incremental_upgrade() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn, &sample()[..1]).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 1);
        run_migrations(&mut conn, &sample()).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);
    }

    #[test]
    fn out_of_order_list_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        let bad = vec![
            Migration {
                version: 2,
                name: "two",
                sql: "",
            },
            Migration {
                version: 1,
                name: "one",
                sql: "",
            },
        ];
        assert!(matches!(
            run_migrations(&mut conn, &bad),
            Err(VaultError::Migration(_))
        ));
    }

    #[test]
    fn duplicate_version_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        let bad = vec![
            Migration {
                version: 1,
                name: "a",
                sql: "",
            },
            Migration {
                version: 1,
                name: "b",
                sql: "",
            },
        ];
        assert!(matches!(
            run_migrations(&mut conn, &bad),
            Err(VaultError::Migration(_))
        ));
    }

    #[test]
    fn unknown_applied_version_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        // 模拟由更新版本的应用写入过的库
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT, applied_at TEXT);
             INSERT INTO schema_migrations (version, name, applied_at) VALUES (99, 'future', 'now');",
        )
        .unwrap();
        assert!(matches!(
            run_migrations(&mut conn, &sample()),
            Err(VaultError::Migration(_))
        ));
    }

    #[test]
    fn failing_migration_rolls_back() {
        let mut conn = Connection::open_in_memory().unwrap();
        let list = vec![
            Migration {
                version: 1,
                name: "ok",
                sql: "CREATE TABLE t_ok (id INTEGER);",
            },
            Migration {
                version: 2,
                name: "bad",
                sql: "CREATE TABLE t_bad (id INTEGER); SYNTAX ERROR;",
            },
        ];
        assert!(matches!(
            run_migrations(&mut conn, &list),
            Err(VaultError::Migration(_))
        ));
        // 失败的迁移回滚：t_bad 不得残留；已成功提交的 v1 不受影响
        let ok_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 't_ok'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ok_tables, 1);
        let bad_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 't_bad'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bad_tables, 0);
        assert_eq!(current_version(&conn).unwrap(), 1);
    }

    #[test]
    fn embedded_migrations_have_unique_increasing_versions() {
        for kind in [DbKind::Meta, DbKind::Tasks, DbKind::Calendar, DbKind::Space] {
            let m = kind.migrations();
            assert!(!m.is_empty(), "{kind:?} 缺少迁移");
            validate(m).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        }
    }

    #[test]
    fn embedded_migrations_apply_to_empty_db() {
        for kind in [DbKind::Meta, DbKind::Tasks, DbKind::Calendar, DbKind::Space] {
            let mut conn = Connection::open_in_memory().unwrap();
            crate::db::connection::configure(&conn).unwrap();
            run_migrations(&mut conn, kind.migrations()).unwrap();
            let expected = kind.migrations().last().unwrap().version;
            assert_eq!(current_version(&conn).unwrap(), expected);
            crate::db::connection::integrity_check(&conn).unwrap();
        }
    }

    /// M3 tasks 0002：FTS 触发器随增删改同步；archived_at 默认 NULL；attachments/taggings 就绪。
    #[test]
    fn tasks_0002_fts_syncs_and_archive_column_defaults_null() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Tasks.migrations()).unwrap();

        conn.execute(
            "INSERT INTO task_lists (id, name) VALUES ('l1', '收件箱')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, list_id, title, notes) VALUES ('t1', 'l1', '写周报', '周一上午完成')",
            [],
        )
        .unwrap();
        let hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks_fts WHERE tasks_fts MATCH '周 报'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1, "插入后 FTS 应同步");

        conn.execute(
            "UPDATE tasks SET notes = '周二下午复核' WHERE id = 't1'",
            [],
        )
        .unwrap();
        let old_hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks_fts WHERE tasks_fts MATCH '完 成'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_hit, 0, "更新后旧内容应移出索引");
        let new_hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks_fts WHERE tasks_fts MATCH '复 核'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(new_hit, 1, "更新后新内容应入索引");

        let archived: Option<String> = conn
            .query_row("SELECT archived_at FROM tasks WHERE id = 't1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(archived.is_none(), "archived_at 应默认 NULL");

        conn.execute("DELETE FROM tasks WHERE id = 't1'", [])
            .unwrap();
        let gone: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(gone, 0, "删除后索引应清空");

        // attachments / taggings 结构就绪
        conn.execute(
            "INSERT INTO attachments (id, entity_type, entity_id, file_name, hash) VALUES ('a1', 'task', 't1', 'f.txt', 'h')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO taggings (tag_id, entity_type, entity_id) VALUES ('tag1', 'task', 't1')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn tasks_0003_preserves_existing_tasks_and_constrains_note_sync_queue() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, &DbKind::Tasks.migrations()[..2]).unwrap();
        conn.execute(
            "INSERT INTO task_lists (id, name) VALUES ('inbox', '收件箱')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, list_id, title) VALUES ('old-task', 'inbox', 'M3 任务')",
            [],
        )
        .unwrap();

        run_migrations(&mut conn, DbKind::Tasks.migrations()).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 4);
        let source: (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT source_page_ref, source_node_id FROM tasks WHERE id = 'old-task'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(source, (None, None), "存量任务必须保持未关联状态");

        conn.execute(
            "INSERT INTO note_sync_queue (id, task_id, source_page_ref, source_node_id, checked)
             VALUES ('q1', 'old-task', 'space-a/page-a', 'node-a', 1)",
            [],
        )
        .unwrap();
        assert!(conn
            .execute(
                "INSERT INTO note_sync_queue (id, task_id, source_page_ref, source_node_id, checked)
                 VALUES ('q2', 'old-task', 'space-a/page-a', 'node-a', 0)",
                [],
            )
            .is_err(), "同一来源队列项必须唯一");
        assert!(
            conn.execute(
                "UPDATE note_sync_queue SET status = 'unknown' WHERE id = 'q1'",
                [],
            )
            .is_err(),
            "队列状态必须受 CHECK 约束"
        );
        assert!(
            conn.execute("UPDATE note_sync_queue SET checked = 2 WHERE id = 'q1'", [],)
                .is_err(),
            "checked 只能是布尔值"
        );
    }

    /// M3 calendar 0002：既有 reminder_minutes 迁入 event_reminders 后旧列废弃；FTS 触发器同步。
    #[test]
    fn calendar_0002_migrates_reminders_and_drops_old_column() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        // 先跑到 0001，构造带 reminder_minutes 的存量数据
        run_migrations(&mut conn, &DbKind::Calendar.migrations()[..1]).unwrap();
        conn.execute(
            "INSERT INTO events (id, title, description, start_at, end_at, reminder_minutes)
             VALUES ('e1', '例会', '讨论进度', '2026-03-02T10:00:00', '2026-03-02T11:00:00', 10)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events (id, title, start_at, end_at) VALUES ('e2', '无提醒', '2026-03-03T10:00:00', '2026-03-03T11:00:00')",
            [],
        )
        .unwrap();

        run_migrations(&mut conn, DbKind::Calendar.migrations()).unwrap();

        let migrated: Vec<(String, i64)> = conn
            .prepare("SELECT event_id, minutes_before FROM event_reminders")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(migrated, vec![("e1".to_string(), 10)], "存量提醒应迁入新表");

        // 旧列已废弃
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(events)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<std::result::Result<Vec<String>, _>>()
            .unwrap();
        assert!(
            !columns.iter().any(|name| name == "reminder_minutes"),
            "reminder_minutes 列应已删除"
        );

        // FTS 回填 + 触发器同步
        let hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events_fts WHERE events_fts MATCH '例 会'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1, "存量事件应回填索引");
        conn.execute(
            "UPDATE events SET description = '改谈排期' WHERE id = 'e1'",
            [],
        )
        .unwrap();
        let new_hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events_fts WHERE events_fts MATCH '排 期'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(new_hit, 1, "更新后 FTS 应同步");
        conn.execute("DELETE FROM events WHERE id = 'e1'", [])
            .unwrap();
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM events_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1, "删除后索引应同步移除");
    }

    #[test]
    fn calendar_0003_preserves_events_and_enforces_external_uids_and_taggings() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, &DbKind::Calendar.migrations()[..2]).unwrap();
        conn.execute(
            "INSERT INTO events (id, title, start_at, end_at) VALUES ('old-event', '既有事件', '2026-03-01T09:00:00', '2026-03-01T10:00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events (id, title, start_at, end_at) VALUES ('other-event', '另一事件', '2026-03-02T09:00:00', '2026-03-02T10:00:00')",
            [],
        )
        .unwrap();

        run_migrations(&mut conn, DbKind::Calendar.migrations()).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 3);
        let uid: Option<String> = conn
            .query_row(
                "SELECT external_uid FROM events WHERE id = 'old-event'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(uid.is_none(), "既有事件不得被补写外部 UID");

        conn.execute(
            "UPDATE events SET external_uid = 'uid-1@example.test' WHERE id = 'old-event'",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "UPDATE events SET external_uid = 'uid-1@example.test' WHERE id = 'other-event'",
                [],
            )
            .is_err(),
            "非空外部 UID 必须唯一"
        );

        conn.execute(
            "INSERT INTO taggings (tag_id, entity_id) VALUES ('tag-work', 'old-event')",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO taggings (tag_id, entity_id) VALUES ('tag-work', 'old-event')",
                [],
            )
            .is_err(),
            "同一事件标签关联必须唯一"
        );
        let tagged: Vec<String> = conn
            .prepare("SELECT entity_id FROM taggings WHERE tag_id = 'tag-work' ORDER BY entity_id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(tagged, vec!["old-event"]);
    }

    #[test]
    fn space_0003_preserves_existing_records_and_limits_templates_to_encrypted_sections() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, &DbKind::Space.migrations()[..2]).unwrap();
        conn.execute(
            "INSERT INTO notebooks (id, name) VALUES ('nb', '笔记本')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sections (id, notebook_id, name, is_encrypted) VALUES ('plain', 'nb', '普通分区', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sections (id, notebook_id, name, is_encrypted) VALUES ('secret', 'nb', '加密分区', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO pages (id, section_id, title, content) VALUES ('page-1', 'plain', '公开页', '可搜索正文')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attachments (id, entity_type, entity_id, file_name, hash) VALUES ('attachment-1', 'page', 'page-1', 'file.txt', 'hash-1')",
            [],
        )
        .unwrap();
        let before_fts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pages_fts WHERE pages_fts MATCH '搜 索'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before_fts, 1);

        run_migrations(&mut conn, &DbKind::Space.migrations()[..3]).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 3);
        let page: String = conn
            .query_row("SELECT content FROM pages WHERE id = 'page-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(page, "可搜索正文");
        let after_fts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pages_fts WHERE pages_fts MATCH '搜 索'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after_fts, 1, "迁移不得影响既有 FTS");
        let attachments: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attachments WHERE id = 'attachment-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(attachments, 1, "迁移不得影响附件登记");

        assert!(conn
            .execute(
                "INSERT INTO section_templates (id, section_id, name_ciphertext, content_ciphertext)
                 VALUES ('plain-template', 'plain', X'01', X'02')",
                [],
            )
            .is_err(), "普通分区不得保存分区加密模板");
        conn.execute(
            "INSERT INTO section_templates (id, section_id, name_ciphertext, content_ciphertext)
             VALUES ('secret-template', 'secret', X'01', X'02')",
            [],
        )
        .unwrap();
    }

    /// M3 meta 0003：reminder_fires 唯一约束去重。
    #[test]
    fn meta_0003_reminder_fires_dedup() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Meta.migrations()).unwrap();

        conn.execute(
            "INSERT INTO reminder_fires (entity_kind, entity_id, occurrence_key, slot) VALUES ('event', 'e1', '2026-03-02T10:00:00', 10)",
            [],
        )
        .unwrap();
        let dup = conn.execute(
            "INSERT INTO reminder_fires (entity_kind, entity_id, occurrence_key, slot) VALUES ('event', 'e1', '2026-03-02T10:00:00', 10)",
            [],
        );
        assert!(dup.is_err(), "同一提醒槽位重复插入应被唯一约束拒绝");
        // 不同槽位/实例互不影响
        conn.execute(
            "INSERT INTO reminder_fires (entity_kind, entity_id, occurrence_key, slot) VALUES ('event', 'e1', '2026-03-02T10:00:00', 1440)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reminder_fires (entity_kind, entity_id, occurrence_key, slot) VALUES ('task', 't1', '', 0)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn meta_0004_creates_templates_and_constrains_m4_preferences() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Meta.migrations()).unwrap();

        assert_eq!(current_version(&conn).unwrap(), 4);
        conn.execute(
            "INSERT INTO page_templates (id, name, content_json) VALUES ('tpl-1', 'Meeting', '{\"type\":\"doc\"}')",
            [],
        )
        .unwrap();
        assert!(conn
            .execute(
                "INSERT INTO page_templates (id, name, content_json) VALUES ('tpl-2', 'meeting', '{\"type\":\"doc\"}')",
                [],
            )
            .is_err(), "模板名称应不区分大小写地唯一");
        assert!(conn
            .execute(
                "INSERT INTO page_templates (id, name, content_json) VALUES ('tpl-3', '坏 JSON', 'not-json')",
                [],
            )
            .is_err(), "模板必须保存有效 JSON");

        let retention: String = conn
            .query_row(
                "SELECT value FROM app_config WHERE key = 'auto_backup_retention_count'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retention, "5");
        assert!(
            conn.execute(
                "UPDATE app_config SET value = 'monthly' WHERE key = 'auto_backup_schedule'",
                [],
            )
            .is_err(),
            "计划频率应受约束"
        );
        assert!(
            conn.execute(
                "UPDATE app_config SET value = '0' WHERE key = 'auto_backup_retention_count'",
                [],
            )
            .is_err(),
            "保留数应受约束"
        );
    }

    #[test]
    fn meta_0004_upgrades_m3_without_overwriting_existing_config_and_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, &DbKind::Meta.migrations()[..3]).unwrap();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('default_space_name', 'M3 工作区')",
            [],
        )
        .unwrap();

        run_migrations(&mut conn, DbKind::Meta.migrations()).unwrap();
        let preserved: String = conn
            .query_row(
                "SELECT value FROM app_config WHERE key = 'default_space_name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(preserved, "M3 工作区");
        assert_eq!(current_version(&conn).unwrap(), 4);

        run_migrations(&mut conn, DbKind::Meta.migrations()).unwrap();
        let migration_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(migration_rows, 4, "重复迁移不得重复记录或覆盖数据");
    }
}
