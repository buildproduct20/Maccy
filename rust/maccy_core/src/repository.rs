use crate::blob_store::{BlobStore, private_dir, regular_file};
use crate::models::*;
use crate::settings::Settings;
use crate::{CoreError, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;

pub struct Store {
    db: Connection,
    blobs: BlobStore,
    settings: Settings,
    trigram: bool,
}

impl Store {
    pub fn open(root: &Path, initial: Option<Settings>, now: i64) -> Result<Self> {
        if let Some(settings) = &initial {
            settings.validate()?;
        }
        private_dir(root)?;
        let path = root.join("history.sqlite3");
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                regular_file(&path).map_err(|_| CoreError::Storage)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)?;
            }
            Err(_) => return Err(CoreError::Storage),
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        // Refuse redirected SQLite sidecars as well as a redirected database.
        for suffix in ["-wal", "-shm", "-journal"] {
            let sidecar = root.join(format!("history.sqlite3{suffix}"));
            if fs::symlink_metadata(&sidecar).is_ok() {
                regular_file(&sidecar).map_err(|_| CoreError::Storage)?;
            }
        }
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_millis(150))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA wal_autocheckpoint=1000;")?;
        let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > 2 {
            return Err(CoreError::Unsupported);
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if version == 1 {
            tx.execute_batch("ALTER TABLE items ADD COLUMN recency_order INTEGER NOT NULL DEFAULT 0; UPDATE items SET recency_order=id;")?;
        }
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings(singleton INTEGER PRIMARY KEY CHECK(singleton=1), value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS items(
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 digest TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK(kind IN ('text','rich','image','files')),
                 preview TEXT NOT NULL, search_text TEXT NOT NULL,
                 restore_text TEXT NOT NULL, advertised TEXT NOT NULL,
                 source_app TEXT, created_at INTEGER NOT NULL,
                 last_copied_at INTEGER NOT NULL, copy_count INTEGER NOT NULL DEFAULT 1 CHECK(copy_count>0),
                 recency_order INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS blobs(digest TEXT PRIMARY KEY, size_bytes INTEGER NOT NULL CHECK(size_bytes>0));
             CREATE TABLE IF NOT EXISTS representations(
                 item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
                 item_index INTEGER NOT NULL, uti TEXT NOT NULL,
                 blob_digest TEXT NOT NULL REFERENCES blobs(digest),
                 PRIMARY KEY(item_id,item_index,uti));
             DROP INDEX IF EXISTS idx_items_recency;
             CREATE INDEX idx_items_recency ON items(last_copied_at DESC,recency_order DESC,id DESC);
             CREATE INDEX IF NOT EXISTS idx_items_expiry ON items(last_copied_at);
             CREATE INDEX IF NOT EXISTS idx_representations_blob ON representations(blob_digest);
             PRAGMA user_version=2;"
        )?;
        let saved: Option<String> = tx
            .query_row("SELECT value FROM settings WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        let settings = if let Some(saved) = saved {
            serde_json::from_str::<Settings>(&saved)?
        } else {
            let settings = initial.unwrap_or_default();
            tx.execute(
                "INSERT INTO settings VALUES(1,?1)",
                [serde_json::to_string(&settings)?],
            )?;
            settings
        };
        settings.validate()?;
        tx.commit()?;
        let trigram = initialize_fts(&db)?;
        let blobs = BlobStore::open(root)?;
        let mut store = Self {
            db,
            blobs,
            settings,
            trigram,
        };
        store.enforce(now)?;
        store.reconcile()?;
        Ok(store)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn count(&self) -> Result<usize> {
        let count: i64 = self
            .db
            .query_row("SELECT count(*) FROM items", [], |row| row.get(0))?;
        usize::try_from(count).map_err(|_| CoreError::Storage)
    }

    pub fn ingest(&mut self, payload: ClipboardPayload, now: i64) -> Result<Option<i64>> {
        // No filesystem operations (including staging) occur before both privacy passes.
        if self.settings.ignores(&payload)? {
            return Ok(None);
        }
        let Some(payload) = normalize(payload, &self.settings)? else {
            return Ok(None);
        };
        if self.settings.ignores(&payload)? {
            return Ok(None);
        }
        let digest = fingerprint(&payload.representations);
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let latest: Option<i64> =
            tx.query_row("SELECT max(last_copied_at) FROM items", [], |r| r.get(0))?;
        let recency = now.max(latest.unwrap_or(now));
        // Keep wall-clock expiry honest while ordering copies within one clock tick.
        let latest_order: i64 = tx.query_row(
            "SELECT coalesce(max(recency_order),0) FROM items",
            [],
            |r| r.get(0),
        )?;
        let recency_order = latest_order.checked_add(1).ok_or(CoreError::Storage)?;
        let duplicate: Option<i64> = tx
            .query_row("SELECT id FROM items WHERE digest=?1", [&digest], |r| {
                r.get(0)
            })
            .optional()?;
        let id = if let Some(id) = duplicate {
            // Verify canonical bytes, not just a hash, before changing a committed row.
            let existing = load_representations(&tx, &self.blobs, id)?;
            if existing != payload.representations {
                return Err(CoreError::Storage);
            }
            tx.execute(
                "UPDATE items SET last_copied_at=?1,recency_order=?2,copy_count=copy_count+1 WHERE id=?3",
                params![recency, recency_order, id],
            )?;
            id
        } else {
            let kind = classify(&payload.representations);
            let preview: String = payload
                .text
                .chars()
                .filter(|&c| c != '\u{fffc}')
                .take(300)
                .collect();
            tx.execute(
                "INSERT INTO items(digest,kind,preview,search_text,restore_text,advertised,source_app,created_at,last_copied_at,recency_order) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![digest,kind,preview,payload.text.to_lowercase(),payload.text,serde_json::to_string(&payload.advertised_types)?,payload.source_app,now,recency,recency_order]
            )?;
            let id = tx.last_insert_rowid();
            for rep in &payload.representations {
                let blob = self.blobs.put(&rep.data)?;
                tx.execute(
                    "INSERT OR IGNORE INTO blobs(digest,size_bytes) VALUES(?1,?2)",
                    params![blob, rep.data.len() as i64],
                )?;
                tx.execute(
                    "INSERT INTO representations VALUES(?1,?2,?3,?4)",
                    params![id, rep.item_index, rep.uti, blob],
                )?;
            }
            id
        };
        retain(&tx, &self.settings, now)?;
        tx.commit()?;
        // Cleanup failure must not misreport an already committed capture as rolled back.
        let _ = self.collect_blobs();
        Ok(Some(id))
    }

    pub fn page(&self, query: &str, cursor: Option<Cursor>, limit: usize) -> Result<Page> {
        let limit = limit.min(PAGE_CAP);
        if limit == 0 {
            return Ok(Page {
                items: vec![],
                next_cursor: None,
            });
        }
        let query = query.to_lowercase();
        let use_fts = self.trigram && query.chars().count() >= 3 && !query.contains('\0');
        let search = if query.is_empty() {
            "1"
        } else if use_fts {
            "id IN (SELECT rowid FROM items_fts WHERE items_fts MATCH ?1) AND instr(search_text,?2)>0"
        } else {
            "search_text LIKE ?1 ESCAPE '\\' AND instr(search_text,?2)>0"
        };
        let sql = format!(
            "SELECT id,kind,preview,source_app,created_at,last_copied_at,copy_count,recency_order FROM items WHERE {search} AND (?3 IS NULL OR (last_copied_at,recency_order,id)<(?3,?4,?5)) ORDER BY last_copied_at DESC,recency_order DESC,id DESC LIMIT ?6"
        );
        let term = if use_fts {
            format!("\"{}\"", query.replace('"', "\"\""))
        } else {
            format!(
                "%{}%",
                query
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            )
        };
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(
            params![
                term,
                query,
                cursor.map(|c| c.last_copied_at),
                cursor.map(|c| c.recency_order),
                cursor.map(|c| c.id),
                (limit + 1) as i64
            ],
            |row| {
                let kind: String = row.get(1)?;
                Ok(ItemSummary {
                    id: row.get(0)?,
                    kind: match kind.as_str() {
                        "rich" => ItemKind::Rich,
                        "image" => ItemKind::Image,
                        "files" => ItemKind::Files,
                        _ => ItemKind::Text,
                    },
                    preview: row.get(2)?,
                    source_app: row.get(3)?,
                    created_at: row.get(4)?,
                    last_copied_at: row.get(5)?,
                    copy_count: row.get::<_, i64>(6)? as u64,
                    recency_order: row.get(7)?,
                })
            },
        )?;
        let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        let more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = if more {
            items.last().map(|item| Cursor {
                last_copied_at: item.last_copied_at,
                recency_order: item.recency_order,
                id: item.id,
            })
        } else {
            None
        };
        Ok(Page { items, next_cursor })
    }

    pub(crate) fn contains(&self, id: i64) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?)
    }

    pub fn payload(&self, id: i64) -> Result<ClipboardPayload> {
        let metadata: Option<(String, String, Option<String>, String)> = self
            .db
            .query_row(
                "SELECT restore_text,advertised,source_app,digest FROM items WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let (text, advertised, source_app, digest) = metadata.ok_or(CoreError::MissingContent)?;
        let representations = load_representations(&self.db, &self.blobs, id)?;
        if representations.is_empty() || fingerprint(&representations) != digest {
            return Err(CoreError::MissingContent);
        }
        Ok(ClipboardPayload {
            representations,
            advertised_types: serde_json::from_str(&advertised)
                .map_err(|_| CoreError::MissingContent)?,
            source_app,
            text,
        })
    }

    pub fn refresh_restored(&mut self, id: i64, now: i64) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (timestamp, order): (i64, i64) = tx.query_row(
            "SELECT coalesce(max(last_copied_at),?1),coalesce(max(recency_order),0) FROM items",
            [now],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let order = order.checked_add(1).ok_or(CoreError::Storage)?;
        if tx.execute("UPDATE items SET last_copied_at=?1,recency_order=?2,copy_count=copy_count+1 WHERE id=?3", params![now.max(timestamp), order, id])? == 0 {
            return Err(CoreError::MissingContent);
        }
        retain(&tx, &self.settings, now)?;
        tx.commit()?;
        let _ = self.collect_blobs();
        Ok(())
    }

    pub fn delete(&mut self, id: i64) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM items WHERE id=?1", [id])?;
        tx.commit()?;
        let _ = self.collect_blobs();
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM items", [])?;
        tx.commit()?;
        let _ = self.collect_blobs();
        Ok(())
    }

    pub fn enforce(&mut self, now: i64) -> Result<CleanupReport> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let deleted_items = retain(&tx, &self.settings, now)?;
        tx.commit()?;
        Ok(CleanupReport {
            deleted_items,
            deleted_blobs: self.collect_blobs().unwrap_or(0),
        })
    }

    pub fn update_settings(&mut self, settings: Settings, now: i64) -> Result<()> {
        settings.validate()?;
        let serialized = serde_json::to_string(&settings)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE settings SET value=?1 WHERE singleton=1",
            [serialized],
        )?;
        retain(&tx, &settings, now)?;
        tx.commit()?;
        self.settings = settings;
        let _ = self.collect_blobs();
        Ok(())
    }

    fn collect_blobs(&mut self) -> Result<usize> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let dead = {
            let mut stmt = tx.prepare("SELECT digest FROM blobs WHERE NOT EXISTS(SELECT 1 FROM representations WHERE blob_digest=blobs.digest)")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut removed = 0;
        for digest in dead {
            if let Ok(deleted) = self.blobs.remove(&digest) {
                removed += usize::from(deleted);
                tx.execute("DELETE FROM blobs WHERE digest=?1", [digest])?;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    fn reconcile(&mut self) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let live = {
            let mut stmt = tx.prepare("SELECT DISTINCT blob_digest FROM representations")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<HashSet<_>, _>>()?
        };
        self.blobs.reconcile(&live)?;
        tx.commit()?;
        Ok(())
    }
}

fn retain(tx: &Transaction<'_>, settings: &Settings, now: i64) -> Result<usize> {
    let cutoff = now.saturating_sub(i64::from(settings.retention_days) * 86_400);
    let expired = tx.execute("DELETE FROM items WHERE last_copied_at<?1", [cutoff])?;
    let over = tx.execute("DELETE FROM items WHERE id IN (SELECT id FROM items ORDER BY last_copied_at DESC,recency_order DESC,id DESC LIMIT -1 OFFSET ?1)",[settings.max_items as i64])?;
    Ok(expired + over)
}

fn load_representations(
    db: &Connection,
    blobs: &BlobStore,
    id: i64,
) -> Result<Vec<Representation>> {
    let mut statement = db.prepare("SELECT item_index,uti,blob_digest,size_bytes FROM representations JOIN blobs ON blobs.digest=blob_digest WHERE item_id=?1 ORDER BY item_index,uti")?;
    let mut rows = statement.query([id])?;
    let mut result = Vec::new();
    let mut total = 0usize;
    while let Some(row) = rows.next()? {
        let size: i64 = row.get(3)?;
        let size = usize::try_from(size).map_err(|_| CoreError::MissingContent)?;
        total = total.checked_add(size).ok_or(CoreError::MissingContent)?;
        if total > MAX_PAYLOAD_BYTES || result.len() >= MAX_REPRESENTATIONS {
            return Err(CoreError::MissingContent);
        }
        result.push(Representation {
            item_index: row.get(0)?,
            uti: row.get(1)?,
            data: blobs.read(&row.get::<_, String>(2)?, size)?,
        });
    }
    Ok(result)
}

fn normalize(
    mut payload: ClipboardPayload,
    settings: &Settings,
) -> Result<Option<ClipboardPayload>> {
    if payload.representations.len() > MAX_REPRESENTATIONS
        || payload.text.len() > MAX_PAYLOAD_BYTES
        || payload.source_app.as_ref().is_some_and(|s| s.len() > 255)
        || payload.advertised_types.len() > 256
    {
        return Ok(None);
    }
    let mut size = payload.text.len();
    for rep in &payload.representations {
        size = match size.checked_add(rep.data.len()) {
            Some(n) if n <= MAX_PAYLOAD_BYTES => n,
            _ => return Ok(None),
        };
        if rep.uti.len() > 255 {
            return Ok(None);
        }
    }
    payload.representations.retain(|rep| {
        SUPPORTED_TYPES.contains(&rep.uti.as_str()) && settings.enabled_types.contains(&rep.uti)
    });
    payload
        .representations
        .sort_by(|a, b| (a.item_index, &a.uti).cmp(&(b.item_index, &b.uti)));
    if payload.representations.is_empty() {
        return Ok(None);
    }
    for (index, rep) in payload.representations.iter().enumerate() {
        if rep.data.is_empty()
            || (index > 0
                && (rep.item_index, &rep.uti)
                    == (
                        payload.representations[index - 1].item_index,
                        &payload.representations[index - 1].uti,
                    ))
        {
            return Ok(None);
        }
        if [TEXT, HTML, FILE_URL].contains(&rep.uti.as_str())
            && std::str::from_utf8(&rep.data).is_err()
        {
            return Ok(None);
        }
        if rep.uti == FILE_URL {
            let url = std::str::from_utf8(&rep.data).map_err(|_| CoreError::InvalidInput)?;
            if !url.starts_with("file:///") && !url.starts_with("file://localhost/") {
                return Ok(None);
            }
            if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
                return Ok(None);
            }
            let bytes = url.as_bytes();
            for i in 0..bytes.len() {
                if bytes[i] == b'%'
                    && (i + 2 >= bytes.len()
                        || !bytes[i + 1].is_ascii_hexdigit()
                        || !bytes[i + 2].is_ascii_hexdigit())
                {
                    return Ok(None);
                }
            }
        }
    }
    if payload.representations.iter().all(|r| r.uti == TEXT)
        && payload
            .representations
            .iter()
            .all(|r| std::str::from_utf8(&r.data).unwrap_or("").trim().is_empty())
    {
        return Ok(None);
    }
    if payload.text.is_empty() {
        payload.text = payload
            .representations
            .iter()
            .filter(|r| r.uti == TEXT || r.uti == FILE_URL)
            .filter_map(|r| {
                std::str::from_utf8(&r.data).ok().map(|text| {
                    if r.uti == FILE_URL {
                        display_url(text)
                    } else {
                        text.into()
                    }
                })
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    payload
        .advertised_types
        .retain(|t| payload.representations.iter().any(|r| &r.uti == t));
    Ok(Some(payload))
}

fn display_url(text: &str) -> String {
    if !text.contains('%') {
        return text.into();
    }
    let mut bytes = Vec::with_capacity(text.len());
    let mut input = text.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
            match (input.next().and_then(hex), input.next().and_then(hex)) {
                (Some(high), Some(low)) => bytes.push((high << 4) | low),
                _ => return text.into(),
            }
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes)
        .ok()
        .filter(|value| !value.chars().any(char::is_control))
        .unwrap_or_else(|| text.into())
}

fn fingerprint(reps: &[Representation]) -> String {
    let mut hash = blake3::Hasher::new();
    hash.update(b"maccy-lite-canonical-v1\0");
    hash.update(&(reps.len() as u64).to_le_bytes());
    for rep in reps {
        hash.update(&rep.item_index.to_le_bytes());
        hash.update(&(rep.uti.len() as u64).to_le_bytes());
        hash.update(rep.uti.as_bytes());
        hash.update(&(rep.data.len() as u64).to_le_bytes());
        hash.update(&rep.data);
    }
    hash.finalize().to_hex().to_string()
}

fn classify(reps: &[Representation]) -> &'static str {
    if reps.iter().any(|r| r.uti == FILE_URL) {
        "files"
    } else if reps.iter().any(|r| IMAGE_TYPES.contains(&r.uti.as_str())) {
        "image"
    } else if reps.iter().any(|r| r.uti == HTML || r.uti == RTF) {
        "rich"
    } else {
        "text"
    }
}

fn initialize_fts(db: &Connection) -> Result<bool> {
    // Probe the actual SQLite tokenizer; LIKE remains valid on builds without it.
    if db.execute_batch("CREATE VIRTUAL TABLE temp.trigram_probe USING fts5(value, tokenize='trigram'); DROP TABLE temp.trigram_probe;").is_err() { return Ok(false); }
    db.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(search_text,content='items',content_rowid='id',tokenize='trigram');
         CREATE TRIGGER IF NOT EXISTS items_fts_insert AFTER INSERT ON items BEGIN INSERT INTO items_fts(rowid,search_text) VALUES(new.id,new.search_text); END;
         CREATE TRIGGER IF NOT EXISTS items_fts_delete AFTER DELETE ON items BEGIN INSERT INTO items_fts(items_fts,rowid,search_text) VALUES('delete',old.id,old.search_text); END;
         CREATE TRIGGER IF NOT EXISTS items_fts_update AFTER UPDATE OF search_text ON items BEGIN INSERT INTO items_fts(items_fts,rowid,search_text) VALUES('delete',old.id,old.search_text); INSERT INTO items_fts(rowid,search_text) VALUES(new.id,new.search_text); END;
         INSERT INTO items_fts(items_fts) VALUES('rebuild');
         COMMIT;"
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn like_fallback_without_trigram() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path(), None, 100).unwrap();
        store
            .ingest(
                ClipboardPayload {
                    representations: vec![Representation {
                        item_index: 0,
                        uti: TEXT.into(),
                        data: b"long %_\\ literal".to_vec(),
                    }],
                    ..Default::default()
                },
                100,
            )
            .unwrap();
        store.trigram = false;
        assert_eq!(store.page("%_\\", None, 50).unwrap().items.len(), 1);
        assert_eq!(store.page("long", None, 50).unwrap().items.len(), 1);
        assert!(store.page("absent", None, 50).unwrap().items.is_empty());
    }
}
