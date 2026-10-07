use super::manifest::*;
use crate::error::VaultResult;
use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct Journal {
    pub conn: Connection,
}
impl Journal {
    pub fn open(root: &Path) -> VaultResult<Self> {
        std::fs::create_dir_all(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        }
        let conn = Connection::open(root.join("transfers.sqlite"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS version(value INTEGER NOT NULL);
          INSERT INTO version SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM version);
          CREATE TABLE IF NOT EXISTS batches(id TEXT PRIMARY KEY,role TEXT NOT NULL,capability TEXT NOT NULL,digest TEXT NOT NULL DEFAULT '',destination TEXT NOT NULL DEFAULT '',output TEXT NOT NULL DEFAULT '',identity TEXT NOT NULL DEFAULT '',phase TEXT NOT NULL DEFAULT 'prepared',updated INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS entries(batch TEXT NOT NULL REFERENCES batches(id) ON DELETE CASCADE,id INTEGER NOT NULL,wire TEXT NOT NULL,name_key TEXT NOT NULL,source TEXT NOT NULL DEFAULT '',signature TEXT NOT NULL DEFAULT '',offset TEXT NOT NULL DEFAULT '0',hash TEXT NOT NULL DEFAULT '',state TEXT NOT NULL DEFAULT 'pending',PRIMARY KEY(batch,id),UNIQUE(batch,name_key));
          CREATE TABLE IF NOT EXISTS chunks(batch TEXT NOT NULL,id INTEGER NOT NULL,offset TEXT NOT NULL,size INTEGER NOT NULL,hash TEXT NOT NULL,PRIMARY KEY(batch,id,offset),FOREIGN KEY(batch,id) REFERENCES entries(batch,id) ON DELETE CASCADE);")?;
        if conn.query_row("SELECT value FROM version", [], |r| r.get::<_, u32>(0))? != 1 {
            return Err(invalid(
                "Unsupported transfer journal version; retain files and use a compatible build",
            ));
        }
        Ok(Self { conn })
    }
    pub fn create(
        &self,
        role: &str,
        id: Option<&str>,
        capability: Option<&str>,
        destination: &str,
    ) -> VaultResult<String> {
        let id = id
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        if uuid::Uuid::parse_str(&id).is_err() {
            return Err(invalid("Invalid batch identity"));
        }
        let capability = match capability {
            Some(s) if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) => s.to_string(),
            Some(_) => return Err(invalid("Invalid resume capability")),
            None => {
                let mut b = [0; 32];
                rand::rngs::OsRng.fill_bytes(&mut b);
                hex(&b)
            }
        };
        self.conn.execute("INSERT INTO batches(id,role,capability,destination,updated) VALUES(?1,?2,?3,?4,unixepoch())",params![id,role,capability,destination])?;
        Ok(id)
    }
    pub fn exists(&self, id: &str) -> VaultResult<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM batches WHERE id=?1", [id], |_| Ok(()))
            .optional()?
            .is_some())
    }
    pub fn field(&self, id: &str, field: &str) -> VaultResult<String> {
        if ![
            "role",
            "capability",
            "digest",
            "destination",
            "output",
            "identity",
            "phase",
        ]
        .contains(&field)
        {
            return Err(invalid("Invalid journal field"));
        }
        self.conn
            .query_row(
                &format!("SELECT {field} FROM batches WHERE id=?1"),
                [id],
                |r| r.get(0),
            )
            .map_err(Into::into)
    }
    pub fn set(&self, id: &str, field: &str, value: &str) -> VaultResult<()> {
        if !["digest", "destination", "output", "identity", "phase"].contains(&field) {
            return Err(invalid("Invalid journal field"));
        }
        self.conn.execute(
            &format!("UPDATE batches SET {field}=?2,updated=unixepoch() WHERE id=?1"),
            params![id, value],
        )?;
        Ok(())
    }
    pub fn add(
        &self,
        batch: &str,
        entry: &Entry,
        source: &str,
        signature: &str,
    ) -> VaultResult<()> {
        path(&entry.path)?;
        self.conn.execute("INSERT INTO entries(batch,id,wire,name_key,source,signature) VALUES(?1,?2,?3,?4,?5,?6)",params![batch,entry.id,String::from_utf8(bytes(entry)?).unwrap(),key(&entry.path),source,signature]).map_err(|error|{
            if matches!(&error,rusqlite::Error::SqliteFailure(code,_) if code.code==rusqlite::ErrorCode::ConstraintViolation){invalid(format!("Selected entries have colliding relative names; rename or remove a selection: {}",entry.path.join("/")))}else{error.into()}
        })?;
        Ok(())
    }
    pub fn entry(&self, batch: &str, id: u32) -> VaultResult<Entry> {
        let s: String = self.conn.query_row(
            "SELECT wire FROM entries WHERE batch=?1 AND id=?2",
            params![batch, id],
            |r| r.get(0),
        )?;
        decode(s.as_bytes())
    }
    pub fn local(
        &self,
        batch: &str,
        id: u32,
    ) -> VaultResult<(String, String, String, u64, String)> {
        let (source, sig, state, offset, hash): (String, String, String, String, String) =
            self.conn.query_row(
                "SELECT source,signature,state,offset,hash FROM entries WHERE batch=?1 AND id=?2",
                params![batch, id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
        Ok((
            source,
            sig,
            state,
            offset
                .parse()
                .map_err(|_| invalid("Invalid stored offset"))?,
            hash,
        ))
    }
    pub fn page(&self, batch: &str, start: u32, limit: u32) -> VaultResult<Vec<Entry>> {
        if limit > 100 {
            return Err(invalid("Preview page exceeds 100 entries"));
        }
        let mut stmt = self
            .conn
            .prepare("SELECT wire FROM entries WHERE batch=?1 AND id>=?2 ORDER BY id LIMIT ?3")?;
        let result = stmt
            .query_map(params![batch, start, limit], |r| r.get::<_, String>(0))?
            .map(|r| decode(r?.as_bytes()))
            .collect();
        result
    }
    pub fn summary(&self, batch: &str) -> VaultResult<Preview> {
        let mut preview = Preview {
            batch: batch.into(),
            digest: self.field(batch, "digest")?,
            destination: self.field(batch, "output")?,
            phase: self.field(batch, "phase")?,
            ..Preview::default()
        };
        let mut stmt = self
            .conn
            .prepare("SELECT wire,state FROM entries WHERE batch=?1 ORDER BY id")?;
        let mut metadata = 0u64;
        let mut recovery = 0u64;
        for row in stmt.query_map([batch], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (wire, state) = row?;
            metadata = metadata
                .checked_add(wire.len() as u64)
                .ok_or_else(|| invalid("Manifest size overflow"))?;
            let entry: Entry = decode(wire.as_bytes())?;
            if entry.id != preview.entries {
                return Err(invalid("Manifest IDs are not sequential"));
            }
            preview.entries += 1;
            match entry.kind {
                Kind::File => {
                    preview.files += 1;
                    preview.total_bytes = preview
                        .total_bytes
                        .checked_add(entry.size)
                        .ok_or_else(|| invalid("Batch size overflow"))?;
                    recovery = recovery
                        .checked_add(
                            entry
                                .size
                                .div_ceil(CHUNK as u64)
                                .checked_mul(128)
                                .ok_or_else(|| invalid("Recovery size overflow"))?,
                        )
                        .ok_or_else(|| invalid("Recovery size overflow"))?;
                }
                Kind::Directory => preview.directories += 1,
                Kind::Excluded => preview.excluded += 1,
            }
            if state == "complete" {
                preview.completed += 1;
            }
        }
        if preview.entries > MAX_ENTRIES
            || metadata > META_LIMIT
            || recovery
                .checked_add(metadata)
                .is_none_or(|n| n > META_LIMIT)
        {
            return Err(invalid(
                "Selection exceeds the 100,000-entry or 64 MiB metadata/recovery budget",
            ));
        }
        Ok(preview)
    }
    pub fn digest(&self, batch: &str) -> VaultResult<String> {
        let mut hash = Sha256::new();
        hash.update(b"tenjee-file-manifest-v1");
        let mut stmt = self
            .conn
            .prepare("SELECT wire FROM entries WHERE batch=?1 ORDER BY id")?;
        for row in stmt.query_map([batch], |r| r.get::<_, String>(0))? {
            let data = bytes(&decode::<Entry>(row?.as_bytes())?)?;
            hash.update((data.len() as u32).to_be_bytes());
            hash.update(data);
        }
        Ok(hex(&hash.finalize()))
    }
    pub fn validate_tree(&self, batch: &str) -> VaultResult<()> {
        let mut rows = self
            .conn
            .prepare("SELECT wire FROM entries WHERE batch=?1 ORDER BY id")?;
        let mut lookup = self
            .conn
            .prepare("SELECT wire FROM entries WHERE batch=?1 AND name_key=?2")?;
        for row in rows.query_map([batch], |r| r.get::<_, String>(0))? {
            let entry: Entry = decode(row?.as_bytes())?;
            for depth in 1..entry.path.len() {
                let parent: String = lookup
                    .query_row(params![batch, key(&entry.path[..depth])], |r| r.get(0))
                    .map_err(|_| invalid("Manifest is missing a parent directory"))?;
                let parent: Entry = decode(parent.as_bytes())?;
                if parent.kind != Kind::Directory || parent.id >= entry.id {
                    return Err(invalid("Manifest parent is not an earlier directory"));
                }
            }
        }
        Ok(())
    }
    pub fn checkpoint(
        &mut self,
        batch: &str,
        id: u32,
        offset: u64,
        chunks: &[(u64, u32, String)],
    ) -> VaultResult<()> {
        let tx = self.conn.transaction()?;
        for (start, size, hash) in chunks {
            tx.execute(
                "INSERT OR REPLACE INTO chunks VALUES(?1,?2,?3,?4,?5)",
                params![batch, id, start.to_string(), size, hash],
            )?;
        }
        tx.execute(
            "UPDATE entries SET offset=?3 WHERE batch=?1 AND id=?2",
            params![batch, id, offset.to_string()],
        )?;
        tx.execute(
            "UPDATE batches SET updated=unixepoch() WHERE id=?1",
            [batch],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn visit_chunks(
        &self,
        batch: &str,
        id: u32,
        mut visit: impl FnMut(u64, u32, String) -> VaultResult<bool>,
    ) -> VaultResult<()> {
        let mut stmt=self.conn.prepare("SELECT offset,size,hash FROM chunks WHERE batch=?1 AND id=?2 ORDER BY length(offset),offset")?;
        let rows = stmt.query_map(params![batch, id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (off, size, hash) = row?;
            if !visit(
                off.parse().map_err(|_| invalid("Invalid checkpoint"))?,
                size,
                hash,
            )? {
                break;
            }
        }
        Ok(())
    }
    pub fn rollback(&self, batch: &str, id: u32, offset: u64) -> VaultResult<()> {
        self.conn.execute("DELETE FROM chunks WHERE batch=?1 AND id=?2 AND (length(offset)>length(?3) OR (length(offset)=length(?3) AND offset>=?3))",params![batch,id,offset.to_string()])?;
        self.conn.execute(
            "UPDATE entries SET offset=?3,state='pending',hash='' WHERE batch=?1 AND id=?2",
            params![batch, id, offset.to_string()],
        )?;
        Ok(())
    }
    pub fn state(&self, batch: &str, id: u32, state: &str, hash: &str) -> VaultResult<()> {
        self.conn.execute(
            "UPDATE entries SET state=?3,hash=?4 WHERE batch=?1 AND id=?2",
            params![batch, id, state, hash],
        )?;
        Ok(())
    }
    pub fn list(&self) -> VaultResult<Vec<Preview>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM batches ORDER BY updated DESC LIMIT 100")?;
        let result = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| self.summary(&r?))
            .collect();
        result
    }
    pub fn delete(&self, batch: &str) -> VaultResult<()> {
        self.conn
            .execute("DELETE FROM batches WHERE id=?1", [batch])?;
        Ok(())
    }
}
pub fn root(app: &tauri::AppHandle) -> VaultResult<PathBuf> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map(|p| p.join("file-exchange"))
        .map_err(|e| invalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_is_separate_durable_and_versioned() {
        let temp = tempfile::tempdir().unwrap();
        let j = Journal::open(temp.path()).unwrap();
        let id = j.create("send", None, None, "").unwrap();
        j.add(
            &id,
            &Entry {
                id: 0,
                path: vec!["a".into()],
                kind: Kind::File,
                size: 1,
                reason: String::new(),
            },
            "source",
            "sig",
        )
        .unwrap();
        let digest = j.digest(&id).unwrap();
        j.set(&id, "digest", &digest).unwrap();
        drop(j);
        let j = Journal::open(temp.path()).unwrap();
        assert_eq!(j.summary(&id).unwrap().total_bytes, 1);
        assert_eq!(j.digest(&id).unwrap(), digest);
        assert_eq!(j.field(&id, "capability").unwrap().len(), 64);
        j.conn.execute("UPDATE version SET value=2", []).unwrap();
        drop(j);
        assert!(Journal::open(temp.path()).is_err());
    }
    #[test]
    fn manifest_budgets_overflow_collision_and_parent_conflicts_fail_before_offer() {
        let temp = tempfile::tempdir().unwrap();
        let j = Journal::open(temp.path()).unwrap();
        let batch = j.create("send", None, None, "").unwrap();
        let entry = |id: u32, name: &str, size: u64| Entry {
            id,
            path: vec![name.into()],
            kind: Kind::File,
            size,
            reason: String::new(),
        };
        j.add(&batch, &entry(0, "É", u64::MAX), "", "").unwrap();
        assert!(j.summary(&batch).is_err());
        j.add(&batch, &entry(1, "other", 1), "", "").unwrap();
        assert!(j
            .summary(&batch)
            .unwrap_err()
            .to_string()
            .contains("overflow"));
        assert!(j
            .add(&batch, &entry(2, "e\u{301}", 0), "", "")
            .unwrap_err()
            .to_string()
            .contains("colliding"));
        assert!(j.page(&batch, 0, 101).is_err());
        let batch = j.create("send", None, None, "").unwrap();
        j.add(
            &batch,
            &Entry {
                id: 0,
                path: vec!["missing".into(), "child".into()],
                kind: Kind::File,
                size: 0,
                reason: String::new(),
            },
            "",
            "",
        )
        .unwrap();
        assert!(j.validate_tree(&batch).is_err());
    }
}
