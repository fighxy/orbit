//! Mailbox database (SQLite). Synchronous; the server calls it from
//! blocking tasks.

use std::path::Path;

use orbit_protocol::mailbox::{Item, ItemId, MailboxId, MailboxStatus, TokenHash};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::config::MailboxConfig;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS mailboxes (
    id            BLOB PRIMARY KEY CHECK (length(id) = 32),
    created_at_ms INTEGER NOT NULL,
    last_seen_ms  INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS deposit_tokens (
    mailbox       BLOB NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    token_hash    BLOB NOT NULL CHECK (length(token_hash) = 32),
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (mailbox, token_hash)
) STRICT;

CREATE TABLE IF NOT EXISTS items (
    seq            INTEGER PRIMARY KEY AUTOINCREMENT,
    mailbox        BLOB NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    id             BLOB NOT NULL CHECK (length(id) = 32),
    received_at_ms INTEGER NOT NULL,
    expires_at_ms  INTEGER NOT NULL,
    size           INTEGER NOT NULL,
    envelope       BLOB NOT NULL,
    UNIQUE (mailbox, id)
) STRICT;

CREATE INDEX IF NOT EXISTS items_by_mailbox ON items (mailbox, seq);
CREATE INDEX IF NOT EXISTS items_by_expiry ON items (expires_at_ms);
";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("mailbox not found")]
    NotFound,
    #[error("quota exceeded: {0}")]
    QuotaExceeded(&'static str),
    #[error("database failure: {0}")]
    Database(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deposited {
    pub duplicate: bool,
    pub expires_at_ms: i64,
}

pub struct Store {
    conn: Connection,
    limits: MailboxConfig,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    pub fn open(path: &Path, limits: MailboxConfig) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        // An acknowledged deposit must survive power loss.
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn, limits })
    }

    pub fn mailbox_exists(&self, mailbox: &MailboxId) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM mailboxes WHERE id = ?1", [mailbox.0.as_slice()], |_| {
                Ok(())
            })
            .optional()?
            .is_some())
    }

    /// Creates the mailbox if needed. Returns true when it was created.
    pub fn ensure_mailbox(&mut self, mailbox: &MailboxId, now_ms: i64) -> Result<bool> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists = tx
            .query_row("SELECT 1 FROM mailboxes WHERE id = ?1", [mailbox.0.as_slice()], |_| {
                Ok(())
            })
            .optional()?
            .is_some();
        if exists {
            tx.execute(
                "UPDATE mailboxes SET last_seen_ms = ?2 WHERE id = ?1",
                params![mailbox.0.as_slice(), now_ms],
            )?;
        } else {
            let count: i64 = tx.query_row("SELECT count(*) FROM mailboxes", [], |row| row.get(0))?;
            if count as u64 >= self.limits.max_mailboxes {
                return Err(StoreError::QuotaExceeded("the node has no room for new mailboxes"));
            }
            tx.execute(
                "INSERT INTO mailboxes (id, created_at_ms, last_seen_ms) VALUES (?1, ?2, ?2)",
                params![mailbox.0.as_slice(), now_ms],
            )?;
        }
        tx.commit()?;
        Ok(!exists)
    }

    pub fn add_token(&mut self, mailbox: &MailboxId, token: &TokenHash, now_ms: i64) -> Result<()> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM deposit_tokens WHERE mailbox = ?1",
            [mailbox.0.as_slice()],
            |row| row.get(0),
        )?;
        let exists = tx
            .query_row(
                "SELECT 1 FROM deposit_tokens WHERE mailbox = ?1 AND token_hash = ?2",
                params![mailbox.0.as_slice(), token.0.as_slice()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            if count >= i64::from(self.limits.max_tokens_per_mailbox) {
                return Err(StoreError::QuotaExceeded("too many deposit tokens"));
            }
            tx.execute(
                "INSERT INTO deposit_tokens (mailbox, token_hash, created_at_ms) VALUES (?1, ?2, ?3)",
                params![mailbox.0.as_slice(), token.0.as_slice(), now_ms],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_token(&mut self, mailbox: &MailboxId, token: &TokenHash) -> Result<()> {
        self.conn.execute(
            "DELETE FROM deposit_tokens WHERE mailbox = ?1 AND token_hash = ?2",
            params![mailbox.0.as_slice(), token.0.as_slice()],
        )?;
        Ok(())
    }

    pub fn token_valid(&self, mailbox: &MailboxId, token: &TokenHash) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM deposit_tokens WHERE mailbox = ?1 AND token_hash = ?2",
                params![mailbox.0.as_slice(), token.0.as_slice()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Stores an envelope. Idempotent per (mailbox, content).
    pub fn deposit(&mut self, mailbox: &MailboxId, envelope: &[u8], now_ms: i64) -> Result<(ItemId, Deposited)> {
        let id = ItemId::of(envelope);
        let ttl_ms = self.ttl_ms();
        let max_items = self.limits.max_items_per_mailbox;
        let max_bytes = self.limits.max_bytes_per_mailbox;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx
            .query_row("SELECT 1 FROM mailboxes WHERE id = ?1", [mailbox.0.as_slice()], |_| {
                Ok(())
            })
            .optional()?
            .is_none()
        {
            return Err(StoreError::NotFound);
        }
        let existing: Option<i64> = tx
            .query_row(
                "SELECT expires_at_ms FROM items WHERE mailbox = ?1 AND id = ?2",
                params![mailbox.0.as_slice(), id.0.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(expires_at_ms) = existing {
            return Ok((
                id,
                Deposited {
                    duplicate: true,
                    expires_at_ms,
                },
            ));
        }
        let (items, bytes): (i64, i64) = tx.query_row(
            "SELECT count(*), coalesce(sum(size), 0) FROM items WHERE mailbox = ?1 AND expires_at_ms > ?2",
            params![mailbox.0.as_slice(), now_ms],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if items as u64 >= max_items {
            return Err(StoreError::QuotaExceeded("mailbox holds too many items"));
        }
        if bytes as u64 + envelope.len() as u64 > max_bytes {
            return Err(StoreError::QuotaExceeded("mailbox holds too many bytes"));
        }
        let expires_at_ms = now_ms.saturating_add(ttl_ms);
        tx.execute(
            "INSERT INTO items (mailbox, id, received_at_ms, expires_at_ms, size, envelope) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                mailbox.0.as_slice(),
                id.0.as_slice(),
                now_ms,
                expires_at_ms,
                envelope.len() as i64,
                envelope
            ],
        )?;
        tx.commit()?;
        Ok((
            id,
            Deposited {
                duplicate: false,
                expires_at_ms,
            },
        ))
    }

    /// Unexpired items after `after_seq`, oldest first.
    pub fn fetch(&self, mailbox: &MailboxId, after_seq: u64, limit: u32, now_ms: i64) -> Result<(Vec<Item>, bool)> {
        let after = i64::try_from(after_seq).unwrap_or(i64::MAX);
        let mut statement = self.conn.prepare(
            "SELECT seq, id, received_at_ms, expires_at_ms, envelope FROM items \
             WHERE mailbox = ?1 AND seq > ?2 AND expires_at_ms > ?3 ORDER BY seq LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![mailbox.0.as_slice(), after, now_ms, i64::from(limit) + 1],
            |row| {
                let id: Vec<u8> = row.get(1)?;
                Ok(Item {
                    seq: row.get::<_, i64>(0)? as u64,
                    id: ItemId(id.as_slice().try_into().unwrap_or([0; 32])),
                    received_at_ms: row.get(2)?,
                    expires_at_ms: row.get(3)?,
                    envelope: row.get(4)?,
                })
            },
        )?;
        let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        let more = items.len() > limit as usize;
        items.truncate(limit as usize);
        Ok((items, more))
    }

    pub fn ack(&mut self, mailbox: &MailboxId, ids: &[ItemId]) -> Result<u32> {
        let tx = self.conn.transaction()?;
        let mut removed = 0;
        {
            let mut statement = tx.prepare("DELETE FROM items WHERE mailbox = ?1 AND id = ?2")?;
            for id in ids {
                removed += statement.execute(params![mailbox.0.as_slice(), id.0.as_slice()])? as u32;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    pub fn status(&self, mailbox: &MailboxId, now_ms: i64) -> Result<MailboxStatus> {
        let (items, bytes): (i64, i64) = self.conn.query_row(
            "SELECT count(*), coalesce(sum(size), 0) FROM items WHERE mailbox = ?1 AND expires_at_ms > ?2",
            params![mailbox.0.as_slice(), now_ms],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let tokens: i64 = self.conn.query_row(
            "SELECT count(*) FROM deposit_tokens WHERE mailbox = ?1",
            [mailbox.0.as_slice()],
            |row| row.get(0),
        )?;
        Ok(MailboxStatus {
            items: items as u64,
            bytes: bytes as u64,
            max_items: self.limits.max_items_per_mailbox,
            max_bytes: self.limits.max_bytes_per_mailbox,
            deposit_tokens: tokens as u32,
            ttl_seconds: self.limits.ttl_seconds,
        })
    }

    /// Deletes expired items; returns how many were removed.
    pub fn sweep(&mut self, now_ms: i64) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM items WHERE expires_at_ms <= ?1", [now_ms])?)
    }

    fn ttl_ms(&self) -> i64 {
        i64::try_from(self.limits.ttl_seconds.saturating_mul(1000)).unwrap_or(i64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(limits: MailboxConfig) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("mailbox.sqlite"), limits).unwrap();
        (dir, store)
    }

    const MB: MailboxId = MailboxId([1; 32]);

    #[test]
    fn deposit_fetch_ack_cycle() {
        let (_dir, mut store) = store(MailboxConfig::default());
        assert!(store.ensure_mailbox(&MB, 0).unwrap());
        assert!(!store.ensure_mailbox(&MB, 1).unwrap());

        let (a, first) = store.deposit(&MB, b"one", 10).unwrap();
        assert!(!first.duplicate);
        let (again, dup) = store.deposit(&MB, b"one", 11).unwrap();
        assert_eq!(a, again);
        assert!(dup.duplicate);
        let (b, _) = store.deposit(&MB, b"two", 12).unwrap();

        let (items, more) = store.fetch(&MB, 0, 1, 20).unwrap();
        assert!(more);
        assert_eq!(items[0].id, a);
        let (rest, more) = store.fetch(&MB, items[0].seq, 10, 20).unwrap();
        assert!(!more);
        assert_eq!(rest[0].id, b);

        assert_eq!(store.ack(&MB, &[a, a]).unwrap(), 1);
        assert_eq!(store.fetch(&MB, 0, 10, 20).unwrap().0.len(), 1);
    }

    #[test]
    fn deposit_requires_existing_mailbox_and_respects_quotas() {
        let limits = MailboxConfig {
            max_items_per_mailbox: 2,
            max_bytes_per_mailbox: 10,
            max_mailboxes: 1,
            ..MailboxConfig::default()
        };
        let (_dir, mut store) = store(limits);
        assert!(matches!(store.deposit(&MB, b"x", 0), Err(StoreError::NotFound)));
        store.ensure_mailbox(&MB, 0).unwrap();
        assert!(matches!(
            store.ensure_mailbox(&MailboxId([2; 32]), 0),
            Err(StoreError::QuotaExceeded(_))
        ));
        assert!(matches!(
            store.deposit(&MB, &[0; 11], 0),
            Err(StoreError::QuotaExceeded(_))
        ));
        store.deposit(&MB, b"a", 0).unwrap();
        store.deposit(&MB, b"b", 0).unwrap();
        assert!(matches!(store.deposit(&MB, b"c", 0), Err(StoreError::QuotaExceeded(_))));
    }

    #[test]
    fn expired_items_are_hidden_and_swept() {
        let limits = MailboxConfig {
            ttl_seconds: 1,
            ..MailboxConfig::default()
        };
        let (_dir, mut store) = store(limits);
        store.ensure_mailbox(&MB, 0).unwrap();
        store.deposit(&MB, b"old", 0).unwrap();
        assert_eq!(store.fetch(&MB, 0, 10, 999).unwrap().0.len(), 1);
        assert!(store.fetch(&MB, 0, 10, 1000).unwrap().0.is_empty());
        assert_eq!(store.sweep(1000).unwrap(), 1);
    }

    #[test]
    fn tokens_are_scoped_to_their_mailbox() {
        let (_dir, mut store) = store(MailboxConfig {
            max_tokens_per_mailbox: 1,
            ..MailboxConfig::default()
        });
        store.ensure_mailbox(&MB, 0).unwrap();
        store.ensure_mailbox(&MailboxId([2; 32]), 0).unwrap();
        let token = TokenHash([9; 32]);
        store.add_token(&MB, &token, 0).unwrap();
        store.add_token(&MB, &token, 0).unwrap(); // idempotent
        assert!(matches!(
            store.add_token(&MB, &TokenHash([8; 32]), 0),
            Err(StoreError::QuotaExceeded(_))
        ));
        assert!(store.token_valid(&MB, &token).unwrap());
        assert!(!store.token_valid(&MailboxId([2; 32]), &token).unwrap());
        store.remove_token(&MB, &token).unwrap();
        assert!(!store.token_valid(&MB, &token).unwrap());
    }
}
