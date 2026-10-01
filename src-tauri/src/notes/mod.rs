//! 笔记领域服务层（design D1）：层级 CRUD、页面读写、附件、加密会话、分区密码生命周期。
//! 所有函数以空间库 `Connection` 为操作对象，command 层负责解析连接；
//! 加密读写按 `sections.is_encrypted` 在此层统一分派（design D3）。

pub mod attachments;
pub mod hierarchy;
pub mod pages;
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
