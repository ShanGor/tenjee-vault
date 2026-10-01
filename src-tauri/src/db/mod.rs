//! 多库分离 SQLite 存储架构（spec: data-storage）。

pub mod connection;
pub mod layout;
pub mod migrate;
pub mod registry;
pub mod restore;
pub mod startup;
