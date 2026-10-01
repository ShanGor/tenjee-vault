//! 统一错误类型：后端所有可预期错误经 `VaultError` 表达，
//! 并可直接序列化为 JSON 返回给前端（Tauri command 错误负载）。

use serde::Serialize;

/// Stable, locale-independent error transport for Tauri commands.  The front end owns the
/// human-facing translation; `params` deliberately remain raw data so user content is never
/// sent through a translation catalogue.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ErrorPayload {
    pub code: &'static str,
    pub params: Vec<(&'static str, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("数据库完整性检查失败: {0}")]
    DbIntegrity(String),

    #[error("迁移失败: {0}")]
    Migration(String),

    #[error("加密错误: {0}")]
    Crypto(String),

    #[error("分区密码错误")]
    WrongPassword,

    #[error("未找到: {0}")]
    NotFound(String),

    #[error("分区已锁定: {0}")]
    SectionLocked(String),

    #[error("参数校验失败: {0}")]
    Validation(String),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQLite 错误: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

impl Serialize for VaultError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Tauri transports command errors as strings. Keep that compatibility while making the
        // string a machine-readable stable payload instead of a localized Rust sentence.
        let payload = self.payload();
        let encoded = serde_json::to_string(&payload).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&encoded)
    }
}

impl VaultError {
    pub fn payload(&self) -> ErrorPayload {
        match self {
            Self::DbIntegrity(detail) => ErrorPayload {
                code: "db_integrity",
                params: vec![("detail", detail.clone())],
            },
            Self::Migration(detail) => ErrorPayload {
                code: "migration",
                params: vec![("detail", detail.clone())],
            },
            Self::Crypto(detail) => ErrorPayload {
                code: "crypto",
                params: vec![("detail", detail.clone())],
            },
            Self::WrongPassword => ErrorPayload {
                code: "wrong_password",
                params: vec![],
            },
            Self::NotFound(detail) => ErrorPayload {
                code: "not_found",
                params: vec![("detail", detail.clone())],
            },
            Self::SectionLocked(section) => ErrorPayload {
                code: "section_locked",
                params: vec![("section", section.clone())],
            },
            Self::Validation(detail) => ErrorPayload {
                code: "validation",
                params: vec![("detail", detail.clone())],
            },
            Self::Io(error) => ErrorPayload {
                code: "io",
                params: vec![("detail", error.to_string())],
            },
            Self::Sqlite(error) => ErrorPayload {
                code: "sqlite",
                params: vec![("detail", error.to_string())],
            },
        }
    }
}

pub type VaultResult<T> = Result<T, VaultError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_stable_code_and_raw_parameters_for_tauri_commands() {
        let json = serde_json::to_string(&VaultError::NotFound("空间 x".into())).unwrap();
        assert_eq!(
            json,
            "\"{\\\"code\\\":\\\"not_found\\\",\\\"params\\\":[[\\\"detail\\\",\\\"空间 x\\\"]]}\""
        );
        let json = serde_json::to_string(&VaultError::WrongPassword).unwrap();
        assert_eq!(
            json,
            "\"{\\\"code\\\":\\\"wrong_password\\\",\\\"params\\\":[]}\""
        );
    }

    #[test]
    fn io_error_converts() {
        let e = VaultError::from(std::io::Error::new(std::io::ErrorKind::NotFound, "x"));
        assert!(matches!(e, VaultError::Io(_)));
    }
}
