//! 笔记领域服务层（design D1）：层级 CRUD、页面读写、附件、加密会话、分区密码生命周期。
//! 所有函数以空间库 `Connection` 为操作对象，command 层负责解析连接；
//! 加密读写按 `sections.is_encrypted` 在此层统一分派（design D3）。

pub mod attachments;
pub mod hierarchy;
pub mod pages;
pub mod page_tree;
pub mod sections_crypto;
pub mod session;

use rusqlite::{params, Connection};

use crate::crypto::cipher;
use crate::error::{VaultError, VaultResult};
use session::SessionManager;

/// 分区是否加密。
pub(crate) fn section_encrypted(conn: &Connection, section_id: &str) -> VaultResult<bool> {
    let encrypted: i64 = conn
        .query_row(
            "SELECT is_encrypted FROM sections WHERE id = ?1",
            params![section_id],
            |r| r.get(0),
        )
        .map_err(|_| VaultError::NotFound(format!("分区 {section_id}")))?;
    Ok(encrypted != 0)
}

/// 加密分区的明文 → 存储表示（base64(v1 tag || ciphertext || nonce)）。
/// 普通分区原样返回。分区锁定（取不到 DSK）时返回 `SectionLocked`。
pub(crate) fn protect_content(
    session: &SessionManager,
    section_id: &str,
    encrypted: bool,
    plaintext: &str,
) -> VaultResult<String> {
    if !encrypted {
        return Ok(plaintext.to_string());
    }
    session.with_dsk(section_id, |dsk| {
        let sealed = cipher::seal(plaintext.as_bytes(), dsk)?;
        Ok(base64_encode(&sealed))
    })
}

/// 存储表示 → 明文（`protect_content` 的逆操作）。
pub(crate) fn reveal_content(
    session: &SessionManager,
    section_id: &str,
    encrypted: bool,
    stored: &str,
) -> VaultResult<String> {
    if !encrypted {
        return Ok(stored.to_string());
    }
    session.with_dsk(section_id, |dsk| {
        // Migrated protected parent pages start as empty documents.
        if stored.is_empty() { return Ok(String::new()); }
        let sealed = base64_decode(stored)?;
        let plain = cipher::open(&sealed, dsk)?;
        String::from_utf8(plain)
            .map_err(|_| VaultError::Crypto("解密结果不是合法 UTF-8 文本".into()))
    })
}

/// 加密分区的明文字节 → 密文字节（附件用）。普通分区原样返回。
pub(crate) fn protect_bytes(
    session: &SessionManager,
    section_id: &str,
    encrypted: bool,
    bytes: &[u8],
) -> VaultResult<Vec<u8>> {
    if !encrypted {
        return Ok(bytes.to_vec());
    }
    session.with_dsk(section_id, |dsk| cipher::seal(bytes, dsk))
}

/// Read titles locally; locked protected titles never escape as storage ciphertext.
pub(crate) fn visible_title(conn: &Connection, session: &SessionManager, id: &str) -> VaultResult<String> {
    let (domain,title,encrypted,flag):(String,String,bool,bool)=conn.query_row(
        "SELECT p.section_id,p.title,s.is_encrypted,p.title_is_encrypted FROM pages p JOIN sections s ON s.id=p.section_id WHERE p.id=?1",
        [id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    if encrypted && !session.is_unlocked(&domain) { return Ok("Protected page".into()); }
    reveal_content(session,&domain,encrypted && flag,&title)
}

/// Legacy protected titles migrate only after a successful local unlock.
pub(crate) fn migrate_titles(conn: &mut Connection, session: &SessionManager, domain: &str) -> VaultResult<()> {
    session.with_dsk(domain, |_|Ok(()))?;
    let tx=conn.transaction()?;
    let rows={let mut stmt=tx.prepare("SELECT id,title FROM pages WHERE section_id=?1 AND title_is_encrypted=0")?;
        let rows=stmt.query_map([domain],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<Result<Vec<_>,_>>()?;rows};
    for (id,title) in rows {
        tx.execute("UPDATE pages SET title=?1,title_is_encrypted=1 WHERE id=?2",params![protect_content(session,domain,true,&title)?,id])?;
    }
    tx.commit()?;Ok(())
}

/// 密文字节 → 明文字节（附件用）。
pub(crate) fn reveal_bytes(
    session: &SessionManager,
    section_id: &str,
    encrypted: bool,
    bytes: &[u8],
) -> VaultResult<Vec<u8>> {
    if !encrypted {
        return Ok(bytes.to_vec());
    }
    session.with_dsk(section_id, |dsk| cipher::open(bytes, dsk))
}

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) fn base64_decode(text: &str) -> VaultResult<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|e| VaultError::Crypto(format!("base64 解码失败: {e}")))
}

pub(crate) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        let blob = vec![0x01, 0xab, 0xff, 0x30];
        assert_eq!(base64_decode(&base64_encode(&blob)).unwrap(), blob);
    }
}
