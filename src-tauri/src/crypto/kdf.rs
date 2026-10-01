//! 密钥派生：分区密码 + 随机 salt → Argon2id → KEK（仅驻留内存）。

use argon2::Argon2;
use rand::rngs::OsRng;
use rand::RngCore;
use zeroize::Zeroizing;

use crate::error::{VaultError, VaultResult};

/// Argon2id KDF 参数（见 spec.md §6：m=64MB, t=3, p=4）。
/// 字段可调以便测试提速与低端设备调参。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KdfParams {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_cost: 64 * 1024, // 64 MB
            t_cost: 3,
            p_cost: 4,
        }
    }
}

impl KdfParams {
    pub fn to_argon2(&self) -> VaultResult<Argon2<'static>> {
        let params = argon2::Params::new(self.m_cost, self.t_cost, self.p_cost, Some(KEK_LEN))
            .map_err(|e| VaultError::Crypto(format!("无效的 KDF 参数: {e}")))?;
        Ok(Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            params,
        ))
    }
}

/// KEK 长度（AES-256 密钥）
pub const KEK_LEN: usize = 32;
/// salt 长度
pub const SALT_LEN: usize = 16;

/// 生成随机 salt（OsRNG）。
pub fn generate_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// 由分区密码派生 KEK。同一密码 + salt + 参数必然派生出相同 KEK。
pub fn derive_kek(
    password: &str,
    salt: &[u8],
    params: &KdfParams,
) -> VaultResult<Zeroizing<[u8; KEK_LEN]>> {
    let argon2 = params.to_argon2()?;
    let mut kek = Zeroizing::new([0u8; KEK_LEN]);
    argon2
        .hash_password_into(password.as_bytes(), salt, kek.as_mut())
        .map_err(|e| VaultError::Crypto(format!("KEK 派生失败: {e}")))?;
    Ok(kek)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用低消耗参数（保持默认参数语义不变的前提下提速）
    fn fast_params() -> KdfParams {
        KdfParams {
            m_cost: 8 * 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    #[test]
    fn same_password_salt_params_derive_same_kek() {
        let salt = generate_salt();
        let params = fast_params();
        let a = derive_kek("correct horse", &salt, &params).unwrap();
        let b = derive_kek("correct horse", &salt, &params).unwrap();
        assert_eq!(a.as_ref(), b.as_ref());
    }

    #[test]
    fn different_password_derives_different_kek() {
        let salt = generate_salt();
        let params = fast_params();
        let a = derive_kek("password-a", &salt, &params).unwrap();
        let b = derive_kek("password-b", &salt, &params).unwrap();
        assert_ne!(a.as_ref(), b.as_ref());
    }

    #[test]
    fn different_salt_derives_different_kek() {
        let params = fast_params();
        let a = derive_kek("same", &generate_salt(), &params).unwrap();
        let b = derive_kek("same", &generate_salt(), &params).unwrap();
        assert_ne!(a.as_ref(), b.as_ref());
    }

    #[test]
    fn default_params_are_spec_values() {
        let p = KdfParams::default();
        assert_eq!((p.m_cost, p.t_cost, p.p_cost), (64 * 1024, 3, 4));
    }

    #[test]
    fn invalid_params_rejected() {
        let bad = KdfParams {
            m_cost: 0,
            t_cost: 0,
            p_cost: 0,
        };
        assert!(bad.to_argon2().is_err());
    }

    #[test]
    fn salt_is_random() {
        assert_ne!(generate_salt(), generate_salt());
    }
}
