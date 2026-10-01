//! 加密分区底层密码学能力（spec: crypto-core）。
//! 两层密钥结构（DSK/KEK）、Argon2id 派生、AES-256-GCM、验证器、密钥不落盘。

pub mod cipher;
pub mod kdf;
pub mod keys;
pub mod password_gen;
