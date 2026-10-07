//! 解锁会话管理（design D2）：`SessionManager` 进程内单例，密钥材料集中持有。
//!
//! - 锁定语义唯一收口：所有读写加密分区的路径经 `with_dsk` / `dsk_copy` 取密钥，
//!   取不到即 `VaultError::SectionLocked`。
//! - 闲置超时：默认 5 分钟（可配置，design D8）；`idle_section_ids` 供巡检，
//!   `lock` 移除条目并由 `SectionKeys` 的 Zeroizing Drop 清零内存。
//! - 修改密码后新 KEK 需要驻留会话：`rewrap` 场景由调用方重新 `unlock`。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::crypto::keys::{unwrap_dsk, SectionKeys, WrappedDsk};
use crate::error::{VaultError, VaultResult};

/// 默认闲置自动锁定时长（分钟）。
pub const DEFAULT_AUTO_LOCK_MINUTES: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
enum UnlockLifetime {
    Idle,
    Page { space_id: String, page_id: String },
    App,
}

struct UnlockedSection {
    keys: SectionKeys,
    last_activity: Instant,
    lifetime: UnlockLifetime,
}

pub struct SessionManager {
    inner: Mutex<HashMap<String, UnlockedSection>>,
    auto_lock: Mutex<Duration>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            auto_lock: Mutex::new(Duration::from_secs(DEFAULT_AUTO_LOCK_MINUTES * 60)),
        }
    }

    /// 解锁：验证器校验密码（错误密码拒绝且不留任何解密能力），
    /// 成功后密钥驻留会话并记录活动时间。
    pub fn unlock_with_password(
        &self,
        section_id: &str,
        wrapped: &WrappedDsk,
        password: &str,
    ) -> VaultResult<()> {
        let keys = unwrap_dsk(wrapped, password)?;
        self.insert(section_id, keys);
        Ok(())
    }

    /// 直接驻留已验证的密钥（设置密码成功后立即解锁的场景）。
    pub fn insert(&self, section_id: &str, keys: SectionKeys) {
        self.inner.lock().unwrap().insert(
            section_id.to_string(),
            UnlockedSection {
                keys,
                last_activity: Instant::now(),
                lifetime: UnlockLifetime::Idle,
            },
        );
    }

    /// Password-based page unlocks use navigation or process exit, rather than idle time.
    pub fn set_unlock_lifetime(&self, section_id: &str, page: Option<(&str, &str)>) {
        if let Some(section) = self.inner.lock().unwrap().get_mut(section_id) {
            section.lifetime = match page {
                Some((space_id, page_id)) => UnlockLifetime::Page {
                    space_id: space_id.into(), page_id: page_id.into(),
                },
                None => UnlockLifetime::App,
            };
        }
    }

    /// Remove page-scoped keys even when navigating to a sibling in the same domain.
    pub fn lock_for_navigation(&self, page: Option<(&str, &str)>) -> Vec<String> {
        let mut map = self.inner.lock().unwrap();
        let mut locked = Vec::new();
        map.retain(|id, section| {
            let keep = match &section.lifetime {
                UnlockLifetime::Page { space_id, page_id } =>
                    page == Some((space_id.as_str(), page_id.as_str())),
                _ => true,
            };
            if !keep { locked.push(id.clone()); }
            keep
        });
        locked
    }

    /// 分区是否处于解锁状态。
    pub fn is_unlocked(&self, section_id: &str) -> bool {
        self.inner.lock().unwrap().contains_key(section_id)
    }

    /// 当前全部解锁分区 id。
    pub fn unlocked_section_ids(&self) -> Vec<String> {
        self.inner.lock().unwrap().keys().cloned().collect()
    }

    /// 锁定分区：移除密钥（Drop 清零）。返回此前是否解锁。
    pub fn lock(&self, section_id: &str) -> bool {
        self.inner.lock().unwrap().remove(section_id).is_some()
    }

    /// 锁定全部（应用退出收口）。返回锁定数量。
    pub fn lock_all(&self) -> usize {
        let mut map = self.inner.lock().unwrap();
        let n = map.len();
        map.clear();
        n
    }

    /// 活动心跳：刷新所有解锁分区的活动时间（设计：用户活动即所有分区不闲置）。
    pub fn touch_all(&self) {
        let mut map = self.inner.lock().unwrap();
        for section in map.values_mut() {
            section.last_activity = Instant::now();
        }
    }

    /// 闲置超时的分区 id（巡检裁决用）。
    pub fn idle_section_ids(&self) -> Vec<String> {
        let timeout = *self.auto_lock.lock().unwrap();
        let now = Instant::now();
        self.inner
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, s)| s.lifetime == UnlockLifetime::Idle && now.duration_since(s.last_activity) >= timeout)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 闲置自动锁定时长。
    pub fn auto_lock_duration(&self) -> Duration {
        *self.auto_lock.lock().unwrap()
    }

    pub fn set_auto_lock_minutes(&self, minutes: u64) {
        *self.auto_lock.lock().unwrap() = Duration::from_secs(minutes.max(1) * 60);
    }

    /// 锁定语义收口：取 DSK 执行闭包。分区未解锁返回 `SectionLocked`。
    /// 取密钥即视为该分区被访问，刷新活动时间。
    pub fn with_dsk<R>(
        &self,
        section_id: &str,
        f: impl FnOnce(&[u8; 32]) -> VaultResult<R>,
    ) -> VaultResult<R> {
        let dsk = {
            let mut map = self.inner.lock().unwrap();
            match map.get_mut(section_id) {
                Some(section) => {
                    section.last_activity = Instant::now();
                    section.keys.dsk.clone()
                }
                None => return Err(VaultError::SectionLocked(section_id.to_string())),
            }
        };
        f(&dsk)
    }

    /// 取得 DSK 副本供批量操作使用（Zeroizing，批量结束后自动清零）。
    pub fn dsk_copy(&self, section_id: &str) -> VaultResult<Zeroizing<[u8; 32]>> {
        self.with_dsk(section_id, |dsk| {
            let mut copy = Zeroizing::new([0u8; 32]);
            copy.copy_from_slice(dsk);
            Ok(copy)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::kdf::KdfParams;
    use crate::crypto::keys::wrap_dsk_with_params;

    fn fast_params() -> KdfParams {
        KdfParams {
            m_cost: 8 * 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    fn wrapped() -> WrappedDsk {
        wrap_dsk_with_params("pw-123", &fast_params()).unwrap().0
    }

    #[test]
    fn wrong_password_rejected_and_not_unlocked() {
        let session = SessionManager::new();
        let w = wrapped();
        assert!(matches!(
            session.unlock_with_password("sec1", &w, "wrong"),
            Err(VaultError::WrongPassword)
        ));
        assert!(!session.is_unlocked("sec1"));
        // 锁定分区取密钥返回 SectionLocked
        assert!(matches!(
            session.with_dsk("sec1", |_| Ok(())),
            Err(VaultError::SectionLocked(_))
        ));
    }

    #[test]
    fn unlock_then_lock_drops_keys() {
        let session = SessionManager::new();
        let w = wrapped();
        session.unlock_with_password("sec1", &w, "pw-123").unwrap();
        assert!(session.is_unlocked("sec1"));
        assert!(session.lock("sec1"));
        assert!(!session.is_unlocked("sec1"));
        assert!(matches!(
            session.with_dsk("sec1", |_| Ok(())),
            Err(VaultError::SectionLocked(_))
        ));
    }

    #[test]
    fn access_refreshes_activity_and_idle_sweep_locks() {
        let session = SessionManager::new();
        let w = wrapped();
        session.unlock_with_password("sec1", &w, "pw-123").unwrap();
        session.set_auto_lock_minutes(1);
        // 直接操纵内部活动时间模拟超时（不真等 60s）
        {
            let mut map = session.inner.lock().unwrap();
            let section = map.get_mut("sec1").unwrap();
            section.last_activity = Instant::now() - Duration::from_secs(120);
        }
        assert_eq!(session.idle_section_ids(), vec!["sec1".to_string()]);
        // touch_all 恢复活性 → 不再闲置
        session.touch_all();
        assert!(session.idle_section_ids().is_empty());
        // 再次超时 → lock 收口
        {
            let mut map = session.inner.lock().unwrap();
            let section = map.get_mut("sec1").unwrap();
            section.last_activity = Instant::now() - Duration::from_secs(120);
        }
        for id in session.idle_section_ids() {
            session.lock(&id);
        }
        assert!(!session.is_unlocked("sec1"));
    }

    #[test]
    fn page_lifetime_locks_on_other_page_space_or_module_but_app_lifetime_survives() {
        let session = SessionManager::new();
        let w = wrapped();
        session.unlock_with_password("page-domain", &w, "pw-123").unwrap();
        session.set_unlock_lifetime("page-domain", Some(("space-a", "page-a")));
        session.unlock_with_password("app-domain", &w, "pw-123").unwrap();
        session.set_unlock_lifetime("app-domain", None);
        {
            let mut map = session.inner.lock().unwrap();
            for section in map.values_mut() {
                section.last_activity = Instant::now() - Duration::from_secs(600);
            }
        }
        assert!(session.idle_section_ids().is_empty());
        assert!(session.lock_for_navigation(Some(("space-a", "page-a"))).is_empty());
        assert_eq!(session.lock_for_navigation(Some(("space-a", "page-b"))), vec!["page-domain"]);
        assert!(session.with_dsk("page-domain", |_| Ok(())).is_err());
        assert!(session.is_unlocked("app-domain"));
        for destination in [Some(("space-b", "page-a")), None] {
            session.unlock_with_password("page-domain", &w, "pw-123").unwrap();
            session.set_unlock_lifetime("page-domain", Some(("space-a", "page-a")));
            assert_eq!(session.lock_for_navigation(destination), vec!["page-domain"]);
            assert!(session.is_unlocked("app-domain"));
        }
        session.lock_all();
        assert!(session.with_dsk("app-domain", |_| Ok(())).is_err());
    }

    #[test]
    fn lock_all_clears_everything() {
        let session = SessionManager::new();
        let w = wrapped();
        session.unlock_with_password("a", &w, "pw-123").unwrap();
        session.unlock_with_password("b", &w, "pw-123").unwrap();
        assert_eq!(session.lock_all(), 2);
        assert!(session.unlocked_section_ids().is_empty());
    }

    #[test]
    fn dsk_copy_matches_unlocked_key() {
        let session = SessionManager::new();
        let (w, dsk) = wrap_dsk_with_params("pw-123", &fast_params()).unwrap();
        session.unlock_with_password("sec1", &w, "pw-123").unwrap();
        let copy = session.dsk_copy("sec1").unwrap();
        assert_eq!(copy.as_ref(), dsk.as_ref());
    }
}
