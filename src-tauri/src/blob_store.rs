//! 内容寻址附件存储助手（design D8）：SHA-256 哈希命名落盘 + 引用计数删除。
//! 笔记附件（`<space>.files/`）与任务附件（`tasks.files/`）共用（纯函数，无加密语义；
//! 加密分区的 seal/open 由调用方在传入字节前后完成）。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use crate::error::VaultResult;

/// 内容哈希（hex）。
pub fn hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 哈希对应的物理文件路径。
pub fn blob_path(dir: &Path, hash: &str) -> PathBuf {
    dir.join(hash)
}

/// 落盘：同哈希只写一次。返回哈希。
pub fn store(dir: &Path, bytes: &[u8]) -> VaultResult<String> {
    let hash = hash_hex(bytes);
    let path = blob_path(dir, &hash);
    if !path.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, bytes)?;
    }
    Ok(hash)
}

/// 读盘。
pub fn read(dir: &Path, hash: &str) -> VaultResult<Vec<u8>> {
    Ok(std::fs::read(blob_path(dir, hash))?)
}

/// 当前哈希的引用计数（attachments 表，两个领域库同构）。
pub fn refcount(conn: &Connection, hash: &str) -> VaultResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM attachments WHERE hash = ?1",
        params![hash],
        |r| r.get(0),
    )?)
}

/// 引用计数归零后物理移除文件（行删除由调用方先行完成）。
pub fn remove_if_unref(dir: &Path, conn: &Connection, hash: &str) -> VaultResult<()> {
    let replicated: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sync_conflicts')", [], |r|r.get(0))?;
    if replicated {
        let retained: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sync_conflicts WHERE entity='attachments' AND json_extract(payload,'$.hash')=?1)", [hash], |r|r.get(0))?;
        if retained { return Ok(()); }
        let groups: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sync_domain_variants')", [], |r|r.get(0))?;
        if groups && conn.query_row("SELECT EXISTS(SELECT 1 FROM sync_domain_variants v,json_tree(v.payload) j WHERE j.key='hash' AND j.value=?1)", [hash], |r|r.get::<_,bool>(0))? { return Ok(()); }
    }
    if refcount(conn, hash)? == 0 {
        let path = blob_path(dir, hash);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_dedups_and_remove_checks_refcount() {
        // 无库依赖的纯文件行为；引用计数路径由领域层测试覆盖
        let dir = tempfile::tempdir().unwrap();
        let h1 = store(dir.path(), b"hello").unwrap();
        let h2 = store(dir.path(), b"hello").unwrap();
        assert_eq!(h1, h2);
        let mut files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        files.sort();
        assert_eq!(files.len(), 1, "同内容只落盘一份");
        assert_eq!(read(dir.path(), &h1).unwrap(), b"hello");
    }
}
