//! Durable public projections and private indexes. Callers supply validated,
//! credential-filtered public objects and SHA-256 hashes of normalized requests.
use crate::{Settings, discovery};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

mod records;
pub use records::{Approval, Decision, Owner, RequestKey};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("unsupported QO database version {0}")]
    Version(i64),
    #[error("invalid replay cursor")]
    InvalidCursor,
    #[error("replay expired (oldest {oldest}, high water {high})")]
    CursorExpired { oldest: u64, high: u64 },
    #[error("ID conflicts with an existing record")]
    Conflict,
    #[error("expected a public event object")]
    InvalidEvent,
}
pub type Result<T> = std::result::Result<T, Error>;

pub struct Store {
    path: PathBuf,
    writer: Mutex<Connection>,
    retention_days: u64,
    retention_bytes: u64,
}

impl Store {
    pub fn open(settings: &Settings) -> Result<Self> {
        Self::open_in(&discovery::state_dir()?, settings)
    }

    fn open_in(dir: &Path, settings: &Settings) -> Result<Self> {
        discovery::private_dir(dir)?;
        let path = dir.join("qo.sqlite3");
        let mut db = Connection::open(&path)?;
        let version = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if !matches!(version, 0 | 1) {
            return Err(Error::Version(version));
        }
        let mode: String = db.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        if mode != "wal" {
            return Err(std::io::Error::other("SQLite WAL unavailable").into());
        }
        if version == 0 {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(include_str!("v1.sql"))?;
            tx.commit()?;
        }
        let store = Self {
            path,
            writer: Mutex::new(db),
            retention_days: settings.event_retention_days,
            retention_bytes: settings.event_retention_bytes,
        };
        // Apply elapsed time and changed limits before the first replay after restart.
        store.write(|_| Ok(()))?;
        Ok(store)
    }

    pub fn write<T>(&self, change: impl FnOnce(&Write<'_>) -> Result<T>) -> Result<T> {
        let mut db = self
            .writer
            .lock()
            .map_err(|_| std::io::Error::other("storage writer unavailable"))?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = change(&Write(View { db: &tx }))?;
        // Prune at every commit. An error can still roll back the entire operation.
        self.prune(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn read<T>(&self, query: impl FnOnce(&View<'_>) -> Result<T>) -> Result<T> {
        let mut db = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let tx = db.transaction()?;
        query(&View { db: &tx })
    }

    fn prune(&self, db: &Connection) -> Result<()> {
        let seconds = self
            .retention_days
            .saturating_mul(86400)
            .min(i64::MAX as u64);
        db.execute(
            "DELETE FROM events WHERE cursor <= (SELECT MAX(cursor) FROM events INDEXED BY events_at
            WHERE at <= unixepoch('now','subsec') - ?)",
            [seconds],
        )?;
        let total: u64 = db.query_row("SELECT retained_bytes FROM metadata", [], |r| r.get(0))?;
        let mut excess = total.saturating_sub(self.retention_bytes);
        if excess > 0 {
            // Scan only the prefix that must go, even for a nearly full log.
            let mut stmt = db.prepare("SELECT cursor, bytes FROM events ORDER BY cursor")?;
            let mut rows = stmt.query([])?;
            let mut last = 0i64;
            while excess > 0 {
                let row = rows.next()?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                last = row.get(0)?;
                excess = excess.saturating_sub(row.get(1)?);
            }
            drop(rows);
            db.execute("DELETE FROM events WHERE cursor <= ?", [last])?;
        }
        Ok(())
    }
}

pub struct View<'a> {
    db: &'a Connection,
}
pub struct Write<'a>(View<'a>);

impl<'a> std::ops::Deref for Write<'a> {
    type Target = View<'a>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Copy)]
pub enum Projection {
    Task,
    Session,
    Turn,
    Item,
    Usage,
}

impl Projection {
    fn table(self) -> &'static str {
        match self {
            Self::Task => "tasks",
            Self::Session => "sessions",
            Self::Turn => "turns",
            Self::Item => "items",
            Self::Usage => "usage",
        }
    }
}

pub struct Snapshot {
    pub cursor: u64,
    pub tasks: Vec<Value>,
    pub sessions: Vec<Value>,
    pub turns: Vec<Value>,
    pub items: Vec<Value>,
    pub usage: Vec<Value>,
}

impl View<'_> {
    pub fn high_water(&self) -> Result<u64> {
        Ok(self
            .db
            .query_row("SELECT high_water FROM metadata", [], |r| r.get(0))?)
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let all = |projection: Projection| -> Result<Vec<Value>> {
            let mut stmt = self
                .db
                .prepare(&format!("SELECT body FROM {}", projection.table()))?;
            Ok(stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?)
        };
        Ok(Snapshot {
            cursor: self.high_water()?,
            tasks: all(Projection::Task)?,
            sessions: all(Projection::Session)?,
            turns: all(Projection::Turn)?,
            items: all(Projection::Item)?,
            usage: all(Projection::Usage)?,
        })
    }

    /// Read (after, through] using the same WAL snapshot for bounds and events.
    pub fn replay(&self, after: u64, through: u64) -> Result<Vec<Value>> {
        let high = self.high_water()?;
        if after > through || through > high {
            return Err(Error::InvalidCursor);
        }
        let first: Option<u64> = self
            .db
            .query_row("SELECT MIN(cursor) FROM events", [], |r| r.get(0))?;
        let oldest = first.unwrap_or(high + 1);
        if after < oldest - 1 {
            return Err(Error::CursorExpired { oldest, high });
        }
        let mut stmt = self
            .db
            .prepare("SELECT body FROM events WHERE cursor > ? AND cursor <= ? ORDER BY cursor")?;
        Ok(stmt
            .query_map(params![after, through], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

impl Write<'_> {
    pub fn put_projection(&self, projection: Projection, public_object: &Value) -> Result<()> {
        self.db.execute(
            &format!(
                "INSERT INTO {} (body) VALUES (?) ON CONFLICT DO UPDATE SET body=excluded.body",
                projection.table()
            ),
            [public_object],
        )?;
        Ok(())
    }

    /// Assign the wire cursor before measuring and storing the encoded event.
    pub fn append_event(&self, mut public_event: Value) -> Result<u64> {
        let cursor = self.high_water()? + 1;
        public_event
            .as_object_mut()
            .ok_or(Error::InvalidEvent)?
            .insert("cursor".into(), Value::String(cursor.to_string()));
        self.db.execute(
            "INSERT INTO events(cursor,body) VALUES (?,?)",
            params![cursor, public_event],
        )?;
        Ok(cursor)
    }
}

#[cfg(test)]
mod tests;
