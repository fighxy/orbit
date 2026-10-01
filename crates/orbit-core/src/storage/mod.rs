//! Durable local storage owned by the engine.
//!
//! Layout: `<data_dir>/accounts/<account_id>/state.sqlite` plus an
//! `engine.lock` file that keeps a second engine from writing the same
//! account concurrently. Platform databases (Room, SwiftData) are not used as
//! independent sources of the same messages.

mod cipher;
mod schema;

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::domain::{
    AccountId, Conversation, ConversationId, ConversationKind, DeviceId, Message, MessageBody, MessageId, MessageState,
    Profile,
};
use crate::error::{Error, Result};
use crate::identity::LocalIdentity;
use crate::limits::MAX_PAGE_SIZE;
use cipher::LocalCipher;

const DATABASE_FILE: &str = "state.sqlite";
const LOCK_FILE: &str = "engine.lock";

/// A page of history in ascending `seq` order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MessagePage {
    pub messages: Vec<Message>,
    /// True when older messages exist before the first returned one.
    pub has_more: bool,
}

pub struct Store {
    conn: Connection,
    cipher: LocalCipher,
    account_id: AccountId,
    device_id: DeviceId,
    saved_messages: ConversationId,
    // Released when the store is dropped.
    _lock: File,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("account_id", &self.account_id)
            .field("device_id", &self.device_id)
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Directory holding the data of one account.
    pub fn account_dir(data_dir: &Path, account: &AccountId) -> PathBuf {
        data_dir.join("accounts").join(account.to_hex())
    }

    pub fn open(data_dir: &Path, identity: &LocalIdentity) -> Result<Self> {
        let public = identity.public();
        let dir = Self::account_dir(data_dir, &public.account_id);
        create_private_dir(&dir)?;

        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK_FILE))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(Error::StorageLocked),
            Err(TryLockError::Error(error)) => return Err(Error::Io(error)),
        }

        let mut conn = Connection::open(dir.join(DATABASE_FILE))?;
        schema::configure(&conn)?;
        let cipher = LocalCipher::new(identity.storage_key());
        let saved_messages = schema::migrate(&mut conn, public, &cipher)?;

        Ok(Self {
            conn,
            cipher,
            account_id: public.account_id,
            device_id: public.device_id,
            saved_messages,
            _lock: lock,
        })
    }

    pub fn saved_messages_id(&self) -> ConversationId {
        self.saved_messages
    }

    pub fn conversations(&self) -> Result<Vec<Conversation>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, kind, created_at_ms FROM conversations ORDER BY created_at_ms, id")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut conversations = Vec::new();
        for row in rows {
            let (id, kind, created_at_ms) = row?;
            let id = ConversationId::from_slice(&id).map_err(|_| Error::Corrupted("conversation id"))?;
            conversations.push(Conversation {
                id,
                kind: ConversationKind::parse(&kind)?,
                created_at_ms,
                last_message: self.last_message(&id)?,
            });
        }
        // Most recently active first; empty conversations keep creation order.
        conversations
            .sort_by_key(|c| std::cmp::Reverse(c.last_message.as_ref().map_or(c.created_at_ms, |m| m.created_at_ms)));
        Ok(conversations)
    }

    pub fn conversation(&self, id: &ConversationId) -> Result<Conversation> {
        self.conversations()?
            .into_iter()
            .find(|c| c.id == *id)
            .ok_or(Error::NotFound("conversation"))
    }

    /// Returns up to `limit` messages older than `before_seq` (or the newest
    /// messages when `before_seq` is `None`), in ascending order.
    pub fn messages(&self, conversation: &ConversationId, before_seq: Option<u64>, limit: u32) -> Result<MessagePage> {
        if limit == 0 || limit > MAX_PAGE_SIZE {
            return Err(Error::InvalidArgument(format!(
                "limit must be within 1..={MAX_PAGE_SIZE}"
            )));
        }
        self.require_conversation(conversation)?;
        let before = match before_seq {
            Some(seq) => i64::try_from(seq).map_err(|_| Error::InvalidArgument("before_seq is too large".into()))?,
            None => i64::MAX,
        };
        let mut statement = self.conn.prepare(
            "SELECT seq, id, conversation_id, author_account, author_device, created_at_ms, \
                    body_nonce, body_ciphertext, state \
             FROM messages WHERE conversation_id = ?1 AND seq < ?2 ORDER BY seq DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![conversation.as_bytes().as_slice(), before, i64::from(limit) + 1],
            StoredMessage::from_row,
        )?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(self.decode(row?)?);
        }
        let has_more = messages.len() > limit as usize;
        messages.truncate(limit as usize);
        messages.reverse();
        Ok(MessagePage { messages, has_more })
    }

    /// Durably stores a text message authored by this device.
    pub fn insert_text(&mut self, conversation: &ConversationId, text: String, created_at_ms: i64) -> Result<Message> {
        self.require_conversation(conversation)?;
        let id = MessageId::random()?;
        let body = MessageBody::Text { text };
        let plaintext = zeroize::Zeroizing::new(
            serde_json::to_vec(&body).map_err(|_| Error::Internal("message body serialization failed"))?,
        );
        let sealed = self.cipher.seal(&id, conversation, &plaintext)?;
        let state = MessageState::SavedLocally;

        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO messages (id, conversation_id, author_account, author_device, created_at_ms, \
                                   body_nonce, body_ciphertext, state) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id.as_bytes().as_slice(),
                conversation.as_bytes().as_slice(),
                self.account_id.as_bytes().as_slice(),
                self.device_id.as_bytes().as_slice(),
                created_at_ms,
                sealed.nonce.as_slice(),
                sealed.ciphertext,
                state.as_str(),
            ],
        )?;
        let seq = tx.last_insert_rowid();
        tx.commit()?;

        Ok(Message {
            id,
            conversation_id: *conversation,
            seq: u64::try_from(seq).map_err(|_| Error::Corrupted("negative message seq"))?,
            author_account: self.account_id,
            author_device: self.device_id,
            created_at_ms,
            body,
            state,
        })
    }

    pub fn profile(&self) -> Result<Option<Profile>> {
        Ok(self
            .conn
            .query_row(
                "SELECT display_name, about, updated_at_ms FROM profile WHERE id = 1",
                [],
                |row| {
                    Ok(Profile {
                        display_name: row.get(0)?,
                        about: row.get(1)?,
                        updated_at_ms: row.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Stores already normalized profile fields.
    pub fn update_profile(&mut self, display_name: String, about: String, updated_at_ms: i64) -> Result<Profile> {
        self.conn.execute(
            "INSERT INTO profile (id, display_name, about, updated_at_ms) VALUES (1, ?1, ?2, ?3) \
             ON CONFLICT (id) DO UPDATE SET display_name = excluded.display_name, \
             about = excluded.about, updated_at_ms = excluded.updated_at_ms",
            params![display_name, about, updated_at_ms],
        )?;
        Ok(Profile {
            display_name,
            about,
            updated_at_ms,
        })
    }

    fn require_conversation(&self, id: &ConversationId) -> Result<()> {
        let exists = self
            .conn
            .query_row(
                "SELECT 1 FROM conversations WHERE id = ?1",
                [id.as_bytes().as_slice()],
                |_| Ok(()),
            )
            .optional()?;
        exists.ok_or(Error::NotFound("conversation"))
    }

    fn last_message(&self, conversation: &ConversationId) -> Result<Option<Message>> {
        let stored = self
            .conn
            .query_row(
                "SELECT seq, id, conversation_id, author_account, author_device, created_at_ms, \
                        body_nonce, body_ciphertext, state \
                 FROM messages WHERE conversation_id = ?1 ORDER BY seq DESC LIMIT 1",
                [conversation.as_bytes().as_slice()],
                StoredMessage::from_row,
            )
            .optional()?;
        stored.map(|m| self.decode(m)).transpose()
    }

    fn decode(&self, stored: StoredMessage) -> Result<Message> {
        let id = MessageId::from_slice(&stored.id).map_err(|_| Error::Corrupted("message id"))?;
        let conversation_id = ConversationId::from_slice(&stored.conversation_id)
            .map_err(|_| Error::Corrupted("message conversation"))?;
        let plaintext = zeroize::Zeroizing::new(self.cipher.open(
            &id,
            &conversation_id,
            &stored.body_nonce,
            &stored.body_ciphertext,
        )?);
        let body: MessageBody =
            serde_json::from_slice(&plaintext).map_err(|_| Error::Corrupted("message body encoding"))?;
        Ok(Message {
            id,
            conversation_id,
            seq: u64::try_from(stored.seq).map_err(|_| Error::Corrupted("negative message seq"))?,
            author_account: AccountId::from_slice(&stored.author_account)
                .map_err(|_| Error::Corrupted("message author account"))?,
            author_device: DeviceId::from_slice(&stored.author_device)
                .map_err(|_| Error::Corrupted("message author device"))?,
            created_at_ms: stored.created_at_ms,
            body,
            state: MessageState::parse(&stored.state)?,
        })
    }
}

struct StoredMessage {
    seq: i64,
    id: Vec<u8>,
    conversation_id: Vec<u8>,
    author_account: Vec<u8>,
    author_device: Vec<u8>,
    created_at_ms: i64,
    body_nonce: Vec<u8>,
    body_ciphertext: Vec<u8>,
    state: String,
}

impl StoredMessage {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            seq: row.get(0)?,
            id: row.get(1)?,
            conversation_id: row.get(2)?,
            author_account: row.get(3)?,
            author_device: row.get(4)?,
            created_at_ms: row.get(5)?,
            body_nonce: row.get(6)?,
            body_ciphertext: row.get(7)?,
            state: row.get(8)?,
        })
    }
}

fn create_private_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
