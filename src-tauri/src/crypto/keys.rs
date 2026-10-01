//! 两层密钥结构：随机分区数据密钥（DSK）加密数据；分区密码派生的 KEK 包裹 DSK。
//! 修改密码只需重包裹 DSK，无需重加密数据。落盘仅存 wrapped DSK、salt、KDF 参数与验证器。

use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use super::cipher;
use super::kdf::{derive_kek, generate_salt, KdfParams};
use crate::error::{VaultError, VaultResult};

/// 验证器固定已知明文：由 KEK 加密存储，解锁时解密比对以判定密码正确性。
const VERIFIER_PLAINTEXT: &[u8] = b"TENJEE_VAULT_SECTION_VERIFIER_V1";

/// 唯一允许落盘的密钥材料集合。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrappedDsk {
    pub salt: Vec<u8>,
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
    /// KEK 加密 VERIFIER_PLAINTEXT 的结果
    pub verifier: Vec<u8>,
    /// KEK 加密 DSK 的结果（格式同 cipher::seal 输出）
    pub wrapped_dsk: Vec<u8>,
}

impl WrappedDsk {
    pub fn kdf_params(&self) -> KdfParams {
        KdfParams {
            m_cost: self.m_cost,
            t_cost: self.t_cost,
            p_cost: self.p_cost,
        }
    }
}

/// 内存中的密钥句柄：kek/dsk 均 Zeroizing 持有，Drop 时自动清零。
pub struct SectionKeys {
    pub kek: Zeroizing<[u8; 32]>,
    pub dsk: Zeroizing<[u8; 32]>,
}

impl Zeroize for SectionKeys {
    fn zeroize(&mut self) {
        self.kek.zeroize();
        self.dsk.zeroize();
    }
}

impl Drop for SectionKeys {
    fn drop(&mut self) {
        self.zeroize();
    }
}

fn generate_dsk() -> Zeroizing<[u8; 32]> {
    let mut dsk = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(dsk.as_mut());
    dsk
}

/// 设置分区密码：生成 DSK 与 salt，派生 KEK，产出 wrapped DSK + 验证器。
pub fn wrap_dsk(password: &str) -> VaultResult<(WrappedDsk, Zeroizing<[u8; 32]>)> {
    wrap_dsk_with_params(password, &KdfParams::default())
}

/// 可指定 KDF 参数的版本（测试提速用）。
pub fn wrap_dsk_with_params(
    password: &str,
    params: &KdfParams,
) -> VaultResult<(WrappedDsk, Zeroizing<[u8; 32]>)> {
    let dsk = generate_dsk();
    let salt = generate_salt();
    let kek = derive_kek(password, &salt, params)?;
    let verifier = cipher::seal(VERIFIER_PLAINTEXT, kek.as_ref())?;
    let wrapped = cipher::seal(dsk.as_ref(), kek.as_ref())?;
    Ok((
        WrappedDsk {
            salt: salt.to_vec(),
            m_cost: params.m_cost,
            t_cost: params.t_cost,
            p_cost: params.p_cost,
            verifier,
            wrapped_dsk: wrapped,
        },
        dsk,
    ))
}

/// 解锁：派生 KEK → 验证器校验 → 解包 DSK。密码错误返回 `VaultError::WrongPassword`。
pub fn unwrap_dsk(wrapped: &WrappedDsk, password: &str) -> VaultResult<SectionKeys> {
    let params = wrapped.kdf_params();
    let kek = derive_kek(password, &wrapped.salt, &params)?;
    // 错误密码派生出的 KEK 无法通过 GCM 认证 → 判定为密码错误
    let verified =
        cipher::open(&wrapped.verifier, kek.as_ref()).map_err(|_| VaultError::WrongPassword)?;
    if verified != VERIFIER_PLAINTEXT {
        // 理论上 GCM 认证已通过即应为已知明文；双保险比对。
        return Err(VaultError::WrongPassword);
    }
    let dsk_bytes = cipher::open(&wrapped.wrapped_dsk, kek.as_ref())?;
    let dsk_vec = Zeroizing::new(dsk_bytes);
    if dsk_vec.len() != 32 {
        return Err(VaultError::Crypto("解包的 DSK 长度非法".into()));
    }
    let mut dsk = Zeroizing::new([0u8; 32]);
    dsk.copy_from_slice(&dsk_vec[..]);
    Ok(SectionKeys { kek, dsk })
}

/// 修改密码：验证旧密码后，用新密码派生的 KEK 重包裹同一 DSK。
pub fn rewrap_dsk(
    wrapped: &WrappedDsk,
    old_password: &str,
    new_password: &str,
) -> VaultResult<WrappedDsk> {
    let keys = unwrap_dsk(wrapped, old_password)?;
    let salt = generate_salt();
    let params = wrapped.kdf_params();
    let new_kek = derive_kek(new_password, &salt, &params)?;
    let verifier = cipher::seal(VERIFIER_PLAINTEXT, new_kek.as_ref())?;
    let new_wrapped = cipher::seal(keys.dsk.as_ref(), new_kek.as_ref())?;
    Ok(WrappedDsk {
        salt: salt.to_vec(),
        m_cost: params.m_cost,
        t_cost: params.t_cost,
        p_cost: params.p_cost,
        verifier,
        wrapped_dsk: new_wrapped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::kdf::KdfParams;

    fn fast_params() -> KdfParams {
        KdfParams {
            m_cost: 8 * 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    fn wrap_fast(password: &str) -> WrappedDsk {
        wrap_dsk_with_params(password, &fast_params()).unwrap().0
    }

    #[test]
    fn init_then_unwrap_succeeds() {
        let (wrapped, dsk) = wrap_dsk_with_params("pw-123", &fast_params()).unwrap();
        assert_eq!(wrapped.salt.len(), crate::crypto::kdf::SALT_LEN);
        let keys = unwrap_dsk(&wrapped, "pw-123").unwrap();
        assert_eq!(keys.dsk.as_ref(), dsk.as_ref());
    }

    #[test]
    fn wrong_password_returns_unlock_error() {
        let wrapped = wrap_fast("right");
        assert!(matches!(
            unwrap_dsk(&wrapped, "wrong"),
            Err(VaultError::WrongPassword)
        ));
    }

    #[test]
    fn wrapped_material_holds_only_safe_fields() {
        let (wrapped, dsk) = wrap_dsk_with_params("pw", &fast_params()).unwrap();
        // 落盘结构体中不包含任何明文密钥字段
        let json = serde_json::to_string(&wrapped).unwrap();
        assert!(!json.contains("kek"));
        let _ = dsk;
    }

    #[test]
    fn rewrap_keeps_same_dsk() {
        let wrapped = wrap_fast("old-pw");
        let before = unwrap_dsk(&wrapped, "old-pw").unwrap();
        let rewrapped = rewrap_dsk(&wrapped, "old-pw", "new-pw").unwrap();
        let after = unwrap_dsk(&rewrapped, "new-pw").unwrap();
        assert_eq!(before.dsk.as_ref(), after.dsk.as_ref());
        // 新 salt，旧密码失效
        assert_ne!(wrapped.salt, rewrapped.salt);
        assert!(matches!(
            unwrap_dsk(&rewrapped, "old-pw"),
            Err(VaultError::WrongPassword)
        ));
    }

    #[test]
    fn rewrap_with_wrong_old_password_fails() {
        let wrapped = wrap_fast("old-pw");
        assert!(matches!(
            rewrap_dsk(&wrapped, "nope", "new-pw"),
            Err(VaultError::WrongPassword)
        ));
    }

    #[test]
    fn unwrapped_keys_can_seal_and_open() {
        let (wrapped, _) = wrap_dsk_with_params("pw", &fast_params()).unwrap();
        let keys = unwrap_dsk(&wrapped, "pw").unwrap();
        let blob = cipher::seal(b"page content", keys.dsk.as_ref()).unwrap();
        assert_eq!(
            cipher::open(&blob, keys.dsk.as_ref()).unwrap(),
            b"page content"
        );
    }

    #[test]
    fn default_params_entrypoint_works() {
        // 覆盖 wrap_dsk 默认参数入口（真实 Argon2id 64MB/3/4 派生）
        let (wrapped, dsk) = wrap_dsk("default-pw-entry").unwrap();
        let keys = unwrap_dsk(&wrapped, "default-pw-entry").unwrap();
        assert_eq!(keys.dsk.as_ref(), dsk.as_ref());
    }

    #[test]
    fn verifier_plaintext_mismatch_rejected() {
        // 构造验证器：GCM 认证可通过但明文不是固定已知明文
        let password = "pw";
        let salt = generate_salt();
        let params = fast_params();
        let kek = derive_kek(password, &salt, &params).unwrap();
        let tampered = WrappedDsk {
            salt: salt.to_vec(),
            m_cost: params.m_cost,
            t_cost: params.t_cost,
            p_cost: params.p_cost,
            verifier: cipher::seal(b"NOT-THE-VERIFIER-PLAINTEXT", kek.as_ref()).unwrap(),
            wrapped_dsk: cipher::seal(&[7u8; 32], kek.as_ref()).unwrap(),
        };
        assert!(matches!(
            unwrap_dsk(&tampered, password),
            Err(VaultError::WrongPassword)
        ));
    }

    #[test]
    fn short_unwrapped_dsk_rejected() {
        // wrapped_dsk 解密成功但长度不是 32 字节
        let password = "pw";
        let salt = generate_salt();
        let params = fast_params();
        let kek = derive_kek(password, &salt, &params).unwrap();
        let bogus = WrappedDsk {
            salt: salt.to_vec(),
            m_cost: params.m_cost,
            t_cost: params.t_cost,
            p_cost: params.p_cost,
            verifier: cipher::seal(VERIFIER_PLAINTEXT, kek.as_ref()).unwrap(),
            wrapped_dsk: cipher::seal(&[1u8, 2, 3], kek.as_ref()).unwrap(),
        };
        assert!(matches!(
            unwrap_dsk(&bogus, password),
            Err(VaultError::Crypto(_))
        ));
    }
}
