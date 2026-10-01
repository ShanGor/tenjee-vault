//! AES-256-GCM 字段加密：每单元随机 nonce，输出格式 `format_tag || ciphertext || nonce`。

use aes_gcm::aead::{Aead, KeyInit, OsRng as AeadOsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::rngs::OsRng;
use rand::RngCore;

use crate::error::{VaultError, VaultResult};

/// 密文格式版本字节（v1），为将来格式演进预留扩展位。
pub const FORMAT_V1: u8 = 0x01;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

fn cipher_from_key(key: &[u8]) -> VaultResult<Aes256Gcm> {
    if key.len() != KEY_LEN {
        return Err(VaultError::Crypto(format!(
            "密钥长度非法: {}，期望 {KEY_LEN}",
            key.len()
        )));
    }
    Ok(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key)))
}

/// 用 DSK/KEK 加密明文，返回 `FORMAT_V1 || ciphertext || nonce`。
pub fn seal(plaintext: &[u8], key: &[u8]) -> VaultResult<Vec<u8>> {
    let cipher = cipher_from_key(key)?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| VaultError::Crypto(format!("加密失败: {e}")))?;
    let mut out = Vec::with_capacity(1 + ciphertext.len() + NONCE_LEN);
    out.push(FORMAT_V1);
    out.extend_from_slice(&ciphertext);
    out.extend_from_slice(&nonce_bytes);
    Ok(out)
}

/// 解密 `FORMAT_V1 || ciphertext || nonce`。格式不符、被篡改或认证失败均返回明确错误。
pub fn open(blob: &[u8], key: &[u8]) -> VaultResult<Vec<u8>> {
    if blob.len() < 1 + NONCE_LEN {
        return Err(VaultError::Crypto("密文过短".into()));
    }
    let (tag, rest) = blob.split_at(1);
    if tag[0] != FORMAT_V1 {
        return Err(VaultError::Crypto(format!(
            "不支持的密文格式版本: {}",
            tag[0]
        )));
    }
    let (ciphertext, nonce_bytes) = rest.split_at(rest.len() - NONCE_LEN);
    let cipher = cipher_from_key(key)?;
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| VaultError::Crypto("解密失败：密文被篡改或密钥错误".into()))
}

/// 便捷函数：直接生成随机加密密钥（32 字节）。
pub fn generate_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    AeadOsRng.fill_bytes(&mut key);
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip() {
        let key = generate_key();
        let plaintext = "服务器账号 deploy notes context".as_bytes();
        let blob = seal(plaintext, &key).unwrap();
        assert_eq!(open(&blob, &key).unwrap(), plaintext);
    }

    #[test]
    fn empty_plaintext_roundtrip() {
        let key = generate_key();
        let blob = seal(b"", &key).unwrap();
        assert_eq!(open(&blob, &key).unwrap(), b"");
    }

    #[test]
    fn output_has_format_tag() {
        let key = generate_key();
        let blob = seal(b"data", &key).unwrap();
        assert_eq!(blob[0], FORMAT_V1);
    }

    #[test]
    fn nonce_is_unique_per_call() {
        let key = generate_key();
        let a = seal(b"same", &key).unwrap();
        let b = seal(b"same", &key).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = generate_key();
        let mut blob = seal(b"secret", &key).unwrap();
        // 篡改密文主体（tag 字节之后的任意字节）
        blob[2] ^= 0x01;
        assert!(matches!(open(&blob, &key), Err(VaultError::Crypto(_))));
    }

    #[test]
    fn tampered_nonce_fails() {
        let key = generate_key();
        let mut blob = seal(b"secret", &key).unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0x01;
        assert!(matches!(open(&blob, &key), Err(VaultError::Crypto(_))));
    }

    #[test]
    fn wrong_key_fails() {
        let blob = seal(b"secret", &generate_key()).unwrap();
        assert!(matches!(
            open(&blob, &generate_key()),
            Err(VaultError::Crypto(_))
        ));
    }

    #[test]
    fn unsupported_format_rejected() {
        let key = generate_key();
        let mut blob = seal(b"data", &key).unwrap();
        blob[0] = 0x7f;
        assert!(matches!(open(&blob, &key), Err(VaultError::Crypto(_))));
    }

    #[test]
    fn wrong_key_length_rejected() {
        assert!(matches!(
            seal(b"data", &[0u8; 16]),
            Err(VaultError::Crypto(_))
        ));
        assert!(matches!(
            open(&[0x01, 0, 0, 0], &[0u8; 16]),
            Err(VaultError::Crypto(_))
        ));
    }

    #[test]
    fn too_short_blob_rejected() {
        let key = generate_key();
        assert!(matches!(open(&[0x01], &key), Err(VaultError::Crypto(_))));
        assert!(matches!(open(&[], &key), Err(VaultError::Crypto(_))));
    }
}
