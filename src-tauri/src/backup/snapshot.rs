//! SQLite online snapshots used by backup creation. The source connection stays open and the
//! SQLite backup API produces a consistent standalone database without copying WAL/SHM files.

use std::path::Path;
use std::time::Duration;

use rusqlite::{backup::Backup, Connection};

use crate::db::connection::{configure, integrity_check};
use crate::error::VaultResult;

pub fn snapshot_connection(source: &Connection, destination: &Path) -> VaultResult<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut target = Connection::open(destination)?;
    configure(&target)?;
    let backup = Backup::new(source, &mut target)?;
    backup.run_to_completion(64, Duration::from_millis(1), None)?;
    drop(backup);
    integrity_check(&target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_snapshot_is_a_complete_database() {
        let root = tempfile::tempdir().unwrap();
        let source = Connection::open(root.path().join("source.db")).unwrap();
        configure(&source).unwrap();
        source.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES (1, 'before');").unwrap();
        let destination = root.path().join("nested/snapshot.db");
        snapshot_connection(&source, &destination).unwrap();
        let copy = Connection::open(destination).unwrap();
        let value: String = copy
            .query_row("SELECT value FROM items WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "before");
        integrity_check(&copy).unwrap();
    }
}
