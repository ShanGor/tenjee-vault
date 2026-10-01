//! SQLite 连接配置：WAL 模式、外键、busy_timeout，以及完整性检查。

use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;
use std::path::Path;

use crate::error::{VaultError, VaultResult};

/// 打开（或创建）数据库连接并应用统一配置：WAL、外键开启、忙等待 5s。
pub fn open_db(path: &Path) -> VaultResult<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    Ok(conn)
}

/// 对既有连接应用统一配置（含 FTS 触发器依赖的 tv_seg 分词函数）。
pub fn configure(conn: &Connection) -> VaultResult<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    register_functions(conn)?;
    Ok(())
}

/// 注册应用层 SQL 标量函数。
///
/// - `tv_seg(text)`：CJK 单字切分，供 0002_fts 触发器把标题/正文写入
///   FTS 索引时使用；查询侧使用同一口径（见 `search::segment_cjk`）。
///   注意：所有会触发 pages_fts 触发器的连接都必须先注册该函数。
fn register_functions(conn: &Connection) -> VaultResult<()> {
    conn.create_scalar_function("tv_seg", 1, FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
        let text: String = ctx.get(0)?;
        Ok(crate::search::segment_cjk(&text))
    })
    .map_err(VaultError::from)
}

/// 快速完整性检查。失败（含文件不可读）返回 `DbIntegrity`，附检查输出的错误内容。
pub fn integrity_check(conn: &Connection) -> VaultResult<()> {
    let check = || -> std::result::Result<Vec<String>, rusqlite::Error> {
        let mut stmt = conn.prepare("PRAGMA integrity_check")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(rows)
    };
    let rows = check().map_err(|e| VaultError::DbIntegrity(format!("无法执行完整性检查: {e}")))?;
    if rows.len() == 1 && rows[0] == "ok" {
        Ok(())
    } else {
        let detail = rows.iter().take(5).cloned().collect::<Vec<_>>().join("; ");
        Err(VaultError::DbIntegrity(detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wal_mode_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(&dir.path().join("wal_test.db")).unwrap();
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn integrity_check_passes_on_fresh_db() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(integrity_check(&conn).is_ok());
    }

    #[test]
    fn integrity_check_fails_on_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.db");
        // 完全非 SQLite 内容：打开阶段即应报错（隔离判定不区分检测点）
        std::fs::write(&path, b"this is not a sqlite database at all").unwrap();
        let open_result = open_db(&path);
        let detected = match open_result {
            Ok(conn) => integrity_check(&conn).is_err(),
            Err(_) => true,
        };
        assert!(detected, "损坏文件必须在打开或完整性检查阶段被识别");
    }

    #[test]
    fn integrity_check_reports_dbintegrity_on_tampered_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tampered.db");
        {
            let conn = open_db(&path).unwrap();
            conn.execute_batch("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1);")
                .unwrap();
        } // 关闭连接，落盘
        let mut bytes = std::fs::read(&path).unwrap();
        // 篡改数据库中段内容（避开前 100 字节的文件头）
        let mid = bytes.len() / 2;
        bytes[mid] = bytes[mid].wrapping_add(0x5a);
        std::fs::write(&path, &bytes).unwrap();
        let conn = open_db(&path).unwrap();
        assert!(matches!(
            integrity_check(&conn),
            Err(VaultError::DbIntegrity(_))
        ));
    }
}
