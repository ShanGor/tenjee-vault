//! 加密分区密码生命周期（spec: 加密分区设置与确认 / 修改与移除分区密码；design D3）。
//!
//! - 设置密码：生成 DSK + KEK 包裹，单事务内把分区既有明文页面批量 seal 回写；
//!   必须先置 `is_encrypted = 1` 再改页面——触发器据此跳过 FTS 索引，避免明文入索引。
//! - 修改密码：仅 `rewrap_dsk` 重包裹 DSK，零数据重写。
//! - 移除密码：验证后单事务内批量解密回明文。
//! - 「忘记密码不可恢复」确认由前端把关，后端强制校验标志位。

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use super::session::SessionManager;
use super::{base64_decode, base64_encode};
use crate::crypto::cipher;
use crate::crypto::kdf::KdfParams;
use crate::crypto::keys::{rewrap_dsk, wrap_dsk_with_params, SectionKeys, WrappedDsk};
use crate::error::{VaultError, VaultResult};

fn load_wrapped(conn: &Connection, section_id: &str) -> VaultResult<WrappedDsk> {
    let row = conn
        .query_row(
            "SELECT kdf_salt, kdf_params, verifier, wrapped_dsk FROM sections WHERE id = ?1 AND is_encrypted = 1",
            params![section_id],
            |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                ))
            },
        )
        .optional()?;
    let (salt, params_json, verifier, wrapped_dsk) =
        row.ok_or_else(|| VaultError::NotFound(format!("加密分区 {section_id}")))?;
    let params: KdfParams = serde_json::from_str(&params_json)
        .map_err(|e| VaultError::Crypto(format!("KDF 参数解析失败: {e}")))?;
    Ok(WrappedDsk {
        salt,
        m_cost: params.m_cost,
        t_cost: params.t_cost,
        p_cost: params.p_cost,
        verifier,
        wrapped_dsk,
    })
}

/// 设置分区密码（含既有页面批量加密迁移，事务内完成）。
/// 返回密钥句柄；调用方应将其驻留会话（设置完成后分区视为已解锁）。
pub fn set_password(
    conn: &mut Connection,
    section_id: &str,
    password: &str,
    confirm_irrecoverable: bool,
) -> VaultResult<SectionKeys> {
    set_password_with_params(
        conn,
        section_id,
        password,
        confirm_irrecoverable,
        &KdfParams::default(),
    )
}

pub fn set_password_with_params(
    conn: &mut Connection,
    section_id: &str,
    password: &str,
    confirm_irrecoverable: bool,
    kdf: &KdfParams,
) -> VaultResult<SectionKeys> {
    if !confirm_irrecoverable {
        return Err(VaultError::Validation(
            "必须确认「忘记密码则数据不可恢复」后才能设置分区密码".into(),
        ));
    }
    let already: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sections WHERE id = ?1 AND is_encrypted = 1",
        params![section_id],
        |r| r.get(0),
    )?;
    if already > 0 {
        return Err(VaultError::Validation("分区已加密".into()));
    }
    let (wrapped, _dsk) = wrap_dsk_with_params(password, kdf)?;
    let keys = crate::crypto::keys::unwrap_dsk(&wrapped, password)?;

    let tx = conn.transaction()?;
    // 先置加密标志：之后的页面 UPDATE 触发器会跳过 FTS（明文不进索引）
    let n = tx.execute(
        "UPDATE sections SET is_encrypted = 1, kdf_salt = ?1, kdf_params = ?2, verifier = ?3, wrapped_dsk = ?4 WHERE id = ?5",
        params![
            wrapped.salt,
            json!({ "m_cost": wrapped.m_cost, "t_cost": wrapped.t_cost, "p_cost": wrapped.p_cost }).to_string(),
            wrapped.verifier,
            wrapped.wrapped_dsk,
            section_id
        ],
    )?;
    if n == 0 {
        return Err(VaultError::NotFound(format!("分区 {section_id}")));
    }
    migrate_pages(&tx, section_id, &keys.dsk, true)?;
    tx.commit()?;
    Ok(keys)
}

/// 修改密码：验证旧密码，仅用新 KEK 重包裹 DSK（不重写任何页面数据）。
pub fn change_password(
    conn: &Connection,
    section_id: &str,
    old_password: &str,
    new_password: &str,
) -> VaultResult<()> {
    let wrapped = load_wrapped(conn, section_id)?;
    let new_wrapped = rewrap_dsk(&wrapped, old_password, new_password)?;
    let n = conn.execute(
        "UPDATE sections SET kdf_salt = ?1, kdf_params = ?2, verifier = ?3, wrapped_dsk = ?4 WHERE id = ?5",
        params![
            new_wrapped.salt,
            json!({ "m_cost": new_wrapped.m_cost, "t_cost": new_wrapped.t_cost, "p_cost": new_wrapped.p_cost }).to_string(),
            new_wrapped.verifier,
            new_wrapped.wrapped_dsk,
            section_id
        ],
    )?;
    if n == 0 {
        return Err(VaultError::NotFound(format!("分区 {section_id}")));
    }
    Ok(())
}

/// 移除密码：验证后单事务内把分区页面解密回明文存储。
pub fn remove_password(
    conn: &mut Connection,
    session: &SessionManager,
    section_id: &str,
    password: &str,
) -> VaultResult<()> {
    // 验证密码（unwrap 失败保持加密状态不变）
    let wrapped = load_wrapped(conn, section_id)?;
    let keys = crate::crypto::keys::unwrap_dsk(&wrapped, password)?;
    let has_templates: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM section_templates WHERE section_id = ?1)",
        [section_id], |row| row.get(0),
    )?;
    if has_templates {
        return Err(VaultError::Validation("请先导出并删除或直接删除此分区的加密模板，再移除密码".into()));
    }
    if !session.is_unlocked(section_id) {
        session.insert(section_id, keys);
    }
    let dsk = session.dsk_copy(section_id)?;
    let tx = conn.transaction()?;
    // 先清除加密标志：之后的页面 UPDATE 触发器会把明文回填进 FTS
    tx.execute(
        "UPDATE sections SET is_encrypted = 0, kdf_salt = NULL, kdf_params = NULL, verifier = NULL, wrapped_dsk = NULL WHERE id = ?1",
        params![section_id],
    )?;
    migrate_pages(&tx, section_id, &dsk, false)?;
    tx.commit()?;
    Ok(())
}

/// 解锁（command 层用）：返回密钥句柄，调用方驻留会话并建内存索引。
pub fn unlock_keys(
    conn: &Connection,
    section_id: &str,
    password: &str,
) -> VaultResult<SectionKeys> {
    let wrapped = load_wrapped(conn, section_id)?;
    crate::crypto::keys::unwrap_dsk(&wrapped, password)
}

/// 批量加/解密分区既有页面（事务内调用）。
/// `encrypt = true`：明文 → base64 密文；`false`：base64 密文 → 明文。
fn migrate_pages(
    conn: &Connection,
    section_id: &str,
    dsk: &[u8; 32],
    encrypt: bool,
) -> VaultResult<()> {
    let mut stmt = conn.prepare("SELECT id, content, title, title_is_encrypted FROM pages WHERE section_id = ?1")?;
    let pages = stmt
        .query_map(params![section_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    let mut update = conn.prepare("UPDATE pages SET content = ?1, title=?3, title_is_encrypted=?4 WHERE id = ?2")?;
    for (page_id, stored,title,flag) in pages {
        let new_stored = if encrypt {
            base64_encode(&cipher::seal(stored.as_bytes(), dsk)?)
        } else {
            let plain = if stored.is_empty() { Vec::new() } else { cipher::open(&base64_decode(&stored)?, dsk)? };
            String::from_utf8(plain)
                .map_err(|_| VaultError::Crypto("解密结果不是合法 UTF-8 文本".into()))?
        };
        let title=if encrypt {base64_encode(&cipher::seal(title.as_bytes(),dsk)?)} else if flag {
            String::from_utf8(cipher::open(&base64_decode(&title)?,dsk)?).map_err(|_|VaultError::Crypto("Invalid title encoding".into()))?
        } else {title};
        update.execute(params![new_stored, page_id,title,encrypt])?;
    }
    Ok(())
}

/// 加密分区全部页面的明文（rowid, title, content）——解锁后建内存临时索引用。
pub fn decrypted_pages(
    conn: &Connection,
    session: &SessionManager,
    section_id: &str,
) -> VaultResult<Vec<(i64, String, String)>> {
    let dsk = session.dsk_copy(section_id)?;
    let mut stmt = conn.prepare(
        "SELECT p.rowid, p.title, p.content, p.title_is_encrypted FROM pages p WHERE p.section_id = ?1 AND p.is_deleted = 0",
    )?;
    let rows = stmt
        .query_map(params![section_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (rowid, title, stored, flag) in rows {
        let title=super::reveal_content(session,section_id,flag,&title)?;
        let plain = if stored.is_empty() { Vec::new() } else { cipher::open(&base64_decode(&stored)?, dsk.as_ref())? };
        let content = String::from_utf8(plain)
            .map_err(|_| VaultError::Crypto("解密结果不是合法 UTF-8 文本".into()))?;
        out.push((rowid, title, content));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::hierarchy;
    use super::super::session::SessionManager;
    use super::*;
    use crate::db::migrate::{run_migrations, DbKind};
    use crate::notes::pages;

    fn fast_params() -> KdfParams {
        KdfParams {
            m_cost: 8 * 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    struct Fx {
        conn: Connection,
        session: SessionManager,
        section_id: String,
    }

    fn fixture() -> Fx {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        let nb = hierarchy::create_notebook(&conn, "nb", None).unwrap();
        let sec = hierarchy::create_section(&conn, &nb.id, None, "s", None).unwrap();
        Fx {
            conn,
            session: SessionManager::new(),
            section_id: sec.id,
        }
    }

    fn add_page(conn: &mut Connection, section_id: &str, id: &str, content: &str) {
        let page = hierarchy::create_page(conn, section_id, None, id).unwrap();
        pages::save_page(conn, &SessionManager::new(), &page.id, id, content).unwrap();
    }

    fn stored_content(conn: &Connection, section_id: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT content FROM pages WHERE section_id = ?1 ORDER BY title")
            .unwrap();
        stmt.query_map(params![section_id], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn set_password_requires_confirmation() {
        let mut fx = fixture();
        assert!(matches!(
            set_password_with_params(&mut fx.conn, &fx.section_id, "pw", false, &fast_params()),
            Err(VaultError::Validation(_))
        ));
        // 保持未加密
        let encrypted: i64 = fx
            .conn
            .query_row(
                "SELECT is_encrypted FROM sections WHERE id = ?1",
                params![fx.section_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(encrypted, 0);
    }

    #[test]
    fn set_password_migrates_existing_pages_and_locks_search_out() {
        let mut fx = fixture();
        add_page(&mut fx.conn, &fx.section_id, "页面一", "机密内容甲");
        add_page(&mut fx.conn, &fx.section_id, "页面二", "机密内容乙");
        let keys =
            set_password_with_params(&mut fx.conn, &fx.section_id, "pw-123", true, &fast_params())
                .unwrap();
        fx.session.insert(&fx.section_id, keys);

        let stored = stored_content(&fx.conn, &fx.section_id);
        assert!(
            stored.iter().all(|c| !c.contains("机密")),
            "既有页面必须整体加密"
        );
        // FTS 持久索引不含加密分区
        let hits = crate::search::search(&fx.conn, "机密", &[], 50).unwrap();
        assert!(hits.is_empty());
        // 解锁后内容可读
        let page_id: String = fx
            .conn
            .query_row("SELECT id FROM pages WHERE title = '页面一'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let page = pages::get_page(&fx.conn, &fx.session, &page_id).unwrap();
        assert_eq!(page.content, "机密内容甲");
    }

    #[test]
    fn change_password_keeps_data_and_rotates_access() {
        let mut fx = fixture();
        add_page(&mut fx.conn, &fx.section_id, "p", "不可变数据");
        let keys =
            set_password_with_params(&mut fx.conn, &fx.section_id, "old-pw", true, &fast_params())
                .unwrap();
        fx.session.insert(&fx.section_id, keys);
        let after_set = stored_content(&fx.conn, &fx.section_id);

        change_password(&fx.conn, &fx.section_id, "old-pw", "new-pw").unwrap();
        let after_change = stored_content(&fx.conn, &fx.section_id);
        assert_eq!(after_set, after_change, "修改密码不得重写页面数据");

        // 旧密码失效、新密码可用
        assert!(matches!(
            unlock_keys(&fx.conn, &fx.section_id, "old-pw"),
            Err(VaultError::WrongPassword)
        ));
        let keys = unlock_keys(&fx.conn, &fx.section_id, "new-pw").unwrap();
        fx.session.lock(&fx.section_id);
        fx.session.insert(&fx.section_id, keys);
        let page_id: String = fx
            .conn
            .query_row("SELECT id FROM pages WHERE title = 'p'", [], |r| r.get(0))
            .unwrap();
        let page = pages::get_page(&fx.conn, &fx.session, &page_id).unwrap();
        assert_eq!(page.content, "不可变数据");

        // 旧密码错误时改密失败且状态不变
        assert!(matches!(
            change_password(&fx.conn, &fx.section_id, "nope", "x"),
            Err(VaultError::WrongPassword)
        ));
    }

    /// 8.3 基准：≥1 万页大分区设置/移除密码耗时（任务约定 >10s 记录数据不阻断）。
    /// 常规 `cargo test` 跳过（#[ignore]），显式 `--ignored` 运行。
    #[test]
    #[ignore]
    fn bench_large_section_set_remove_password() {
        use std::time::Instant;
        let mut fx = fixture();
        const PAGES: usize = 10_000;
        let tx = fx.conn.transaction().unwrap();
        for i in 0..PAGES {
            let page_id = format!("page-{i:05}");
            tx.execute(
                "INSERT INTO pages (id, section_id, title, content) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    page_id,
                    fx.section_id,
                    format!("页面 {i}"),
                    format!("第 {i} 页的正文内容，包含一些用于加密迁移的文本。")
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();

        let t0 = Instant::now();
        let keys = set_password_with_params(
            &mut fx.conn,
            &fx.section_id,
            "pw-bench",
            true,
            &fast_params(),
        )
        .unwrap();
        fx.session.insert(&fx.section_id, keys);
        let set_elapsed = t0.elapsed();

        let t1 = Instant::now();
        remove_password(&mut fx.conn, &fx.session, &fx.section_id, "pw-bench").unwrap();
        let remove_elapsed = t1.elapsed();

        println!(
            "bench_large_section({PAGES} pages): set={set_elapsed:?}, remove={remove_elapsed:?}"
        );
        assert!(
            set_elapsed.as_secs() < 60 && remove_elapsed.as_secs() < 60,
            "基准异常缓慢"
        );
    }

    #[test]
    fn remove_password_restores_plaintext() {
        let mut fx = fixture();
        add_page(&mut fx.conn, &fx.section_id, "p", "回明文的内容");
        let keys =
            set_password_with_params(&mut fx.conn, &fx.section_id, "pw-123", true, &fast_params())
                .unwrap();
        fx.session.insert(&fx.section_id, keys);

        remove_password(&mut fx.conn, &fx.session, &fx.section_id, "pw-123").unwrap();
        let stored = stored_content(&fx.conn, &fx.section_id);
        assert_eq!(stored, vec!["回明文的内容".to_string()]);
        let encrypted: i64 = fx
            .conn
            .query_row(
                "SELECT is_encrypted FROM sections WHERE id = ?1",
                params![fx.section_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(encrypted, 0);
        // 明文重新可搜
        let hits = crate::search::search(&fx.conn, "回明文", &[], 50).unwrap();
        assert_eq!(hits.len(), 1);

        // 错误密码移除失败，加密状态保持
        let keys =
            set_password_with_params(&mut fx.conn, &fx.section_id, "pw-2", true, &fast_params())
                .unwrap();
        fx.session.insert(&fx.section_id, keys);
        assert!(matches!(
            remove_password(&mut fx.conn, &fx.session, &fx.section_id, "wrong"),
            Err(VaultError::WrongPassword)
        ));
        let encrypted: i64 = fx
            .conn
            .query_row(
                "SELECT is_encrypted FROM sections WHERE id = ?1",
                params![fx.section_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(encrypted, 1);
    }
}
