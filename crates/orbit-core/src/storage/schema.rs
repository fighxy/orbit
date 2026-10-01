//! Schema creation, migrations and ownership checks of the account database.

use rusqlite::{Connection, ErrorCode as SqliteCode, OptionalExtension, TransactionBehavior, params};

use super::cipher::LocalCipher;
use crate::domain::{ConversationId, ConversationKind};
use crate::error::{Error, Result};
use crate::identity::PublicIdentity;

pub(super) const CURRENT_VERSION: i64 = 1;

const META_ACCOUNT_ID: &str = "account_id";
const META_DEVICE_ID: &str = "device_id";
const META_KEY_CHECK: &str = "storage_key_check";
const META_SAVED_MESSAGES: &str = "saved_messages_conversation_id";

const SCHEMA_V1: &str = "
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
) STRICT;

CREATE TABLE conversations (
    id            BLOB PRIMARY KEY CHECK (length(id) = 16),
    kind          TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE messages (
    seq             INTEGER PRIMARY KEY AUTOINCREMENT,
    id              BLOB NOT NULL UNIQUE CHECK (length(id) = 16),
    conversation_id BLOB NOT NULL REFERENCES conversations(id),
    author_account  BLOB NOT NULL CHECK (length(author_account) = 32),
    author_device   BLOB NOT NULL CHECK (length(author_device) = 32),
    created_at_ms   INTEGER NOT NULL,
    body_nonce      BLOB NOT NULL,
    body_ciphertext BLOB NOT NULL,
    state           TEXT NOT NULL
) STRICT;

CREATE INDEX messages_by_conversation ON messages (conversation_id, seq);
";

/// Connection settings. `synchronous = FULL` keeps a committed message on
/// power loss in WAL mode, which is what a confirmed local state promises.
pub(super) fn configure(conn: &Connection) -> Result<()> {
    let journal_mode: String = conn
        .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
        .map_err(map_open_error)?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(Error::Internal("storage does not support WAL journal mode"));
    }
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// Creates or upgrades the schema and verifies that the database belongs to
/// `identity`. Returns the ID of the saved-messages conversation.
pub(super) fn migrate(
    conn: &mut Connection,
    identity: &PublicIdentity,
    cipher: &LocalCipher,
) -> Result<ConversationId> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match version {
        0 => initialize(conn, identity, cipher),
        CURRENT_VERSION => verify_owner(conn, identity, cipher),
        other => Err(Error::UnsupportedStorageVersion(other)),
    }
}

fn initialize(conn: &mut Connection, identity: &PublicIdentity, cipher: &LocalCipher) -> Result<ConversationId> {
    let saved_messages = ConversationId::random()?;
    let created_at_ms = crate::engine::now_ms();

    let tx = conn.transaction_with_behavior(TransactionBehavior::Exclusive)?;
    tx.execute_batch(SCHEMA_V1)?;
    let mut insert_meta = tx.prepare("INSERT INTO meta (key, value) VALUES (?1, ?2)")?;
    insert_meta.execute(params![META_ACCOUNT_ID, identity.account_id.as_bytes().as_slice()])?;
    insert_meta.execute(params![META_DEVICE_ID, identity.device_id.as_bytes().as_slice()])?;
    insert_meta.execute(params![META_KEY_CHECK, cipher.key_check().as_slice()])?;
    insert_meta.execute(params![META_SAVED_MESSAGES, saved_messages.as_bytes().as_slice()])?;
    drop(insert_meta);
    tx.execute(
        "INSERT INTO conversations (id, kind, created_at_ms) VALUES (?1, ?2, ?3)",
        params![
            saved_messages.as_bytes().as_slice(),
            ConversationKind::SavedMessages.as_str(),
            created_at_ms
        ],
    )?;
    tx.pragma_update(None, "user_version", CURRENT_VERSION)?;
    tx.commit()?;
    Ok(saved_messages)
}

fn verify_owner(conn: &Connection, identity: &PublicIdentity, cipher: &LocalCipher) -> Result<ConversationId> {
    let account = read_meta(conn, META_ACCOUNT_ID)?;
    let device = read_meta(conn, META_DEVICE_ID)?;
    if account != identity.account_id.as_bytes() || device != identity.device_id.as_bytes() {
        return Err(Error::IdentityMismatch);
    }
    cipher.verify_key_check(&read_meta(conn, META_KEY_CHECK)?)?;
    let saved = read_meta(conn, META_SAVED_MESSAGES)?;
    ConversationId::from_slice(&saved).map_err(|_| Error::Corrupted("saved messages conversation id"))
}

fn read_meta(conn: &Connection, key: &'static str) -> Result<Vec<u8>> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get(0))
        .optional()?
        .ok_or(Error::Corrupted("storage metadata is missing"))
}

fn map_open_error(error: rusqlite::Error) -> Error {
    match error.sqlite_error_code() {
        Some(SqliteCode::NotADatabase) | Some(SqliteCode::DatabaseCorrupt) => {
            Error::Corrupted("account database is not a valid SQLite database")
        }
        _ => Error::Storage(error),
    }
}
