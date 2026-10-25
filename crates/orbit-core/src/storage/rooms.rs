//! Pairwise groups and channels.
//!
//! Each other member receives a separately sealed copy. This is not MLS:
//! there is no shared epoch, and someone who already decrypted a copy keeps it.
//! A channel uses the same fan-out; only the owner can publish.

use std::collections::HashSet;

use orbit_protocol::NodeAddress;
use orbit_protocol::envelope::{ContactCard, DeviceIdentity, MAX_ROOM_MEMBERS, MIN_ROOM_MEMBERS, Payload, RoomKind};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::Store;
use super::cipher::{NONCE_LEN, SealedBody};
use super::delivery::{self, IncomingRevision};
use crate::domain::{
    AccountId, Conversation, ConversationId, ConversationKind, DeviceId, Message, MessageBody, MessageId, MessageState,
    normalize_profile, normalize_text,
};
use crate::error::{Error, Result};
use crate::identity::LocalIdentity;

const MAX_ROOMS: i64 = 100;
const MAX_HELD: i64 = 64;

struct RoomRecord {
    kind: ConversationKind,
    owner: [u8; 32],
    members: Vec<ContactCard>,
}

pub(super) struct StoredMember {
    pub device: [u8; 32],
    pub sealed: SealedBody,
}

pub(super) struct WelcomeStore {
    pub conversation: ConversationId,
    pub kind: ConversationKind,
    pub title: String,
    pub owner: [u8; 32],
    pub created_at_ms: i64,
    pub members: Vec<StoredMember>,
}

pub(super) struct IncomingWelcome {
    pub room_id: [u8; 16],
    pub kind: RoomKind,
    pub title: String,
    pub members: Vec<ContactCard>,
}

pub(super) struct IncomingRoomText {
    pub conversation: ConversationId,
    pub id: MessageId,
    pub sealed: Option<SealedBody>,
    pub reply: Option<(ContactCard, Payload)>,
}

pub(super) struct HeldRoomText {
    pub id: MessageId,
    pub room_id: ConversationId,
    pub author_account: AccountId,
    pub author_device: DeviceId,
    pub created_at_ms: i64,
    pub sealed: SealedBody,
}

pub(super) struct HeldRoomEdit {
    pub id: MessageId,
    pub room_id: ConversationId,
    pub author_account: AccountId,
    pub author_device: DeviceId,
    pub revision: u32,
    pub deleted: bool,
    pub edited_at_ms: i64,
    pub sealed: SealedBody,
}

pub(super) enum RoomTextPlan {
    Incoming(Box<IncomingRoomText>),
    Hold(HeldRoomText),
}

pub(super) enum RoomRevisionPlan {
    Now(Option<IncomingRevision>),
    Later(HeldRoomEdit),
}

pub(super) struct BodyRevision<'a> {
    pub conversation: &'a ConversationId,
    pub sender: &'a DeviceIdentity,
    pub message_id: [u8; 16],
    pub revision: u32,
    pub text: Option<String>,
    pub edited_at_ms: i64,
}

impl Store {
    pub(super) fn room_presentation(
        &self,
        id: &ConversationId,
        kind: ConversationKind,
    ) -> Result<(Option<String>, bool)> {
        match kind {
            ConversationKind::SavedMessages | ConversationKind::Direct => Ok((None, true)),
            ConversationKind::Group | ConversationKind::Channel => {
                let (title, owner, _) = self
                    .room_meta(id)?
                    .ok_or(Error::Corrupted("room is missing its membership"))?;
                let can_post = kind == ConversationKind::Group || owner == *self.device_id.as_bytes();
                Ok((Some(title), can_post))
            }
        }
    }

    pub(crate) fn create_room(
        &mut self,
        identity: &LocalIdentity,
        kind: ConversationKind,
        title: &str,
        member_conversations: &[ConversationId],
        now: i64,
    ) -> Result<Conversation> {
        let (title, _) = normalize_profile(title, "")?;
        if member_conversations.is_empty() {
            return Err(Error::InvalidArgument("choose at least one contact".into()));
        }
        if member_conversations.len() + 1 > MAX_ROOM_MEMBERS {
            return Err(Error::InvalidArgument(format!(
                "a room can include at most {MAX_ROOM_MEMBERS} members"
            )));
        }
        let mut seen = HashSet::new();
        let mut cards = vec![self.own_card(identity)?];
        for conversation in member_conversations {
            if !seen.insert(*conversation) {
                return Err(Error::InvalidArgument("duplicate member".into()));
            }
            let (card, ready) = self.contact_card(conversation)?.ok_or(Error::NotFound("contact"))?;
            if !ready {
                return Err(Error::InvalidArgument("contact exchange is not finished".into()));
            }
            if cards.iter().any(|old| old.identity.device == card.identity.device) {
                return Err(Error::InvalidArgument("duplicate member".into()));
            }
            cards.push(card);
        }
        self.ensure_room_capacity()?;
        let id = ConversationId::random()?;
        let payload = Payload::RoomWelcome {
            room_id: *id.as_bytes(),
            kind: protocol_kind(kind)?,
            title: title.clone(),
            members: cards.clone(),
        };
        let jobs = seal_fanout(self, identity, &cards, now, &payload)?;
        let members = seal_members(self, &id, &cards)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_room(&tx, &id, kind, &title, self.device_id.as_bytes(), now, &members)?;
        for (job, envelope, route) in &jobs {
            delivery::insert_outbox(&tx, job, &id, None, envelope, route)?;
        }
        tx.commit()?;
        self.conversation(&id)
    }

    pub(crate) fn queue_room_text(
        &mut self,
        identity: &LocalIdentity,
        room: &ConversationId,
        text: String,
        now: i64,
    ) -> Result<Message> {
        let record = self.load_room(room)?.ok_or(Error::NotFound("conversation"))?;
        self.ensure_can_publish(&record)?;
        let id = MessageId::random()?;
        let payload = Payload::RoomText {
            room_id: *room.as_bytes(),
            message_id: *id.as_bytes(),
            text: text.clone(),
        };
        let jobs = seal_fanout(self, identity, &record.members, now, &payload)?;
        let body = MessageBody::Text { text };
        let sealed = delivery::seal_body(self, &id, room, &body)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        delivery::insert_message(
            &tx,
            &id,
            room,
            &self.account_id,
            &self.device_id,
            now,
            &sealed,
            MessageState::Queued,
        )?;
        let seq = tx.last_insert_rowid() as u64;
        for (job, envelope, route) in &jobs {
            delivery::insert_outbox(&tx, job, room, Some(&id), envelope, route)?;
        }
        tx.commit()?;
        Ok(Message {
            id,
            conversation_id: *room,
            seq,
            author_account: self.account_id,
            author_device: self.device_id,
            created_at_ms: now,
            body,
            state: MessageState::Queued,
            revision: 0,
            edited_at_ms: None,
            deleted: false,
        })
    }

    pub(super) fn room_revision_envelopes(
        &self,
        identity: &LocalIdentity,
        room: &ConversationId,
        message_id: &MessageId,
        revision: u32,
        text: Option<&str>,
        now: i64,
    ) -> Result<Vec<(MessageId, Vec<u8>, SealedBody)>> {
        let record = self.load_room(room)?.ok_or(Error::NotFound("conversation"))?;
        self.ensure_can_publish(&record)?;
        let payload = match text {
            Some(value) => Payload::RoomEditText {
                room_id: *room.as_bytes(),
                message_id: *message_id.as_bytes(),
                revision,
                text: value.to_owned(),
            },
            None => Payload::RoomDeleteText {
                room_id: *room.as_bytes(),
                message_id: *message_id.as_bytes(),
                revision,
            },
        };
        seal_fanout(self, identity, &record.members, now, &payload)
    }

    pub(super) fn plan_welcome(
        &self,
        identity: &LocalIdentity,
        sender: &DeviceIdentity,
        incoming: IncomingWelcome,
        now: i64,
    ) -> Result<Option<WelcomeStore>> {
        let IncomingWelcome {
            room_id,
            kind,
            title,
            members,
        } = incoming;
        let (title, _) =
            normalize_profile(&title, "").map_err(|_| Error::InvalidArgument("invalid room title".into()))?;
        if !(MIN_ROOM_MEMBERS..=MAX_ROOM_MEMBERS).contains(&members.len()) {
            return Err(Error::InvalidArgument("invalid room membership".into()));
        }
        let room = ConversationId::from_bytes(room_id);
        let mut devices = HashSet::new();
        let mut includes_self = false;
        for card in &members {
            if !devices.insert(card.identity.device) {
                return Err(Error::InvalidArgument("duplicate member".into()));
            }
            if card.identity.device == *self.device_id.as_bytes() {
                if card.identity != identity.public().to_device_identity()
                    || card.inbox_key != identity.inbox().public()
                {
                    return Err(Error::InvalidArgument(
                        "room membership does not match this device".into(),
                    ));
                }
                includes_self = true;
                continue;
            }
            card.identity
                .verify()
                .map_err(|_| Error::InvalidArgument("invalid member certificate".into()))?;
            card.node
                .parse::<NodeAddress>()
                .map_err(|_| Error::InvalidArgument("invalid member address".into()))?;
            normalize_profile(&card.display_name, "")
                .map_err(|_| Error::InvalidArgument("invalid member name".into()))?;
            if card.identity.account == *self.account_id.as_bytes() {
                return Err(Error::InvalidArgument(
                    "room lists another device of this account".into(),
                ));
            }
        }
        if !includes_self || !members.iter().any(|card| card.identity == *sender) {
            return Err(Error::InvalidArgument("room welcome is not from a member".into()));
        }
        let local_kind = local_kind(kind);
        if let Some((stored_title, owner, stored_kind)) = self.room_meta(&room)? {
            let stored = self.member_cards(&room)?;
            if stored_kind == local_kind
                && stored_title == title
                && owner == sender.device
                && same_membership(&stored, &members)
            {
                return Ok(None);
            }
            return Err(Error::InvalidArgument("room membership conflicts".into()));
        }
        self.ensure_room_capacity()?;
        Ok(Some(WelcomeStore {
            conversation: room,
            kind: local_kind,
            title,
            owner: sender.device,
            created_at_ms: now,
            members: seal_members(self, &room, &members)?,
        }))
    }

    pub(super) fn plan_room_text(
        &self,
        sender: &DeviceIdentity,
        sent_at_ms: i64,
        room_id: [u8; 16],
        message_id: [u8; 16],
        text: &str,
    ) -> Result<RoomTextPlan> {
        let room = ConversationId::from_bytes(room_id);
        let id = MessageId::from_bytes(message_id);
        let text = normalize_text(text)?;
        let Some(record) = self.load_room(&room)? else {
            let body = MessageBody::Text { text };
            return Ok(RoomTextPlan::Hold(HeldRoomText {
                id,
                room_id: room,
                author_account: AccountId::from_bytes(sender.account),
                author_device: DeviceId::from_bytes(sender.device),
                created_at_ms: sent_at_ms,
                sealed: delivery::seal_body(self, &id, &room, &body)?,
            }));
        };
        let card = member_card(&record, sender)?;
        if record.kind == ConversationKind::Channel && sender.device != record.owner {
            return Err(Error::InvalidArgument("only the channel owner can publish".into()));
        }
        let reply = Some((
            card.clone(),
            Payload::Delivered {
                message_ids: vec![message_id],
            },
        ));
        if let Some(old) = self.lookup_message(&id)? {
            let same_author = old.conversation_id == room
                && old.author_device.as_bytes() == &sender.device
                && old.author_account.as_bytes() == &sender.account;
            let same_text = old.body == (MessageBody::Text { text: text.clone() });
            if !same_author || (!same_text && old.revision == 0 && !old.deleted) {
                return Err(Error::InvalidArgument("conflicting message id".into()));
            }
            return Ok(RoomTextPlan::Incoming(Box::new(IncomingRoomText {
                conversation: room,
                id,
                sealed: None,
                reply,
            })));
        }
        let body = MessageBody::Text { text };
        Ok(RoomTextPlan::Incoming(Box::new(IncomingRoomText {
            conversation: room,
            id,
            sealed: Some(delivery::seal_body(self, &id, &room, &body)?),
            reply,
        })))
    }

    pub(super) fn plan_room_revision(
        &self,
        sender: &DeviceIdentity,
        sent_at_ms: i64,
        room_id: [u8; 16],
        message_id: [u8; 16],
        revision: u32,
        text: Option<String>,
    ) -> Result<RoomRevisionPlan> {
        let room = ConversationId::from_bytes(room_id);
        let Some(record) = self.load_room(&room)? else {
            if revision == 0 {
                return Ok(RoomRevisionPlan::Now(None));
            }
            let text = text.as_deref().map(normalize_text).transpose()?;
            let deleted = text.is_none();
            let body = match &text {
                Some(value) => MessageBody::Text { text: value.clone() },
                None => MessageBody::Deleted,
            };
            let id = MessageId::from_bytes(message_id);
            return Ok(RoomRevisionPlan::Later(HeldRoomEdit {
                id,
                room_id: room,
                author_account: AccountId::from_bytes(sender.account),
                author_device: DeviceId::from_bytes(sender.device),
                revision,
                deleted,
                edited_at_ms: sent_at_ms,
                sealed: delivery::seal_body(self, &id, &room, &body)?,
            }));
        };
        member_card(&record, sender)?;
        if record.kind == ConversationKind::Channel && sender.device != record.owner {
            return Err(Error::InvalidArgument("only the channel owner can publish".into()));
        }
        self.plan_body_revision(BodyRevision {
            conversation: &room,
            sender,
            message_id,
            revision,
            text,
            edited_at_ms: sent_at_ms,
        })
        .map(RoomRevisionPlan::Now)
    }

    pub(super) fn room_receipts_for(&self, sender: &DeviceIdentity, ids: &[MessageId]) -> Result<Vec<MessageId>> {
        if sender.device == *self.device_id.as_bytes() {
            return Ok(Vec::new());
        }
        let mut accepted = Vec::new();
        for id in ids {
            let Some(message) = self.lookup_message(id)? else {
                continue;
            };
            if message.author_device != self.device_id || message.author_account != self.account_id {
                continue;
            }
            let Some(record) = self.load_room(&message.conversation_id)? else {
                continue;
            };
            if record.members.iter().any(|card| card.identity == *sender) {
                accepted.push(*id);
            }
        }
        Ok(accepted)
    }

    fn ensure_room_capacity(&self) -> Result<()> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM rooms", [], |row| row.get(0))?;
        if count >= MAX_ROOMS {
            return Err(Error::InvalidArgument("too many rooms".into()));
        }
        Ok(())
    }

    fn ensure_can_publish(&self, room: &RoomRecord) -> Result<()> {
        let mine = *self.device_id.as_bytes();
        if !room.members.iter().any(|card| card.identity.device == mine) {
            return Err(Error::InvalidArgument("you are not a member of this room".into()));
        }
        if room.kind == ConversationKind::Channel && room.owner != mine {
            return Err(Error::InvalidArgument("only the channel owner can publish".into()));
        }
        Ok(())
    }

    fn load_room(&self, room: &ConversationId) -> Result<Option<RoomRecord>> {
        let Some((.., owner, kind)) = self.room_meta(room)? else {
            return Ok(None);
        };
        Ok(Some(RoomRecord {
            kind,
            owner,
            members: self.member_cards(room)?,
        }))
    }

    fn room_meta(&self, room: &ConversationId) -> Result<Option<(String, [u8; 32], ConversationKind)>> {
        let row = self
            .conn
            .query_row(
                "SELECT r.title, r.owner_device, c.kind FROM rooms r \
                 JOIN conversations c ON c.id = r.conversation_id WHERE r.conversation_id = ?1",
                [room.as_bytes().as_slice()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(title, owner, kind)| {
            let owner: [u8; 32] = owner
                .as_slice()
                .try_into()
                .map_err(|_| Error::Corrupted("room owner"))?;
            Ok((title, owner, ConversationKind::parse(&kind)?))
        })
        .transpose()
    }

    fn member_cards(&self, room: &ConversationId) -> Result<Vec<ContactCard>> {
        let mut statement = self
            .conn
            .prepare("SELECT device_id, nonce, ciphertext FROM room_members WHERE conversation_id = ?1")?;
        let rows = statement.query_map([room.as_bytes().as_slice()], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut cards = Vec::new();
        for row in rows {
            let (device, nonce, ciphertext) = row?;
            let device: [u8; 32] = device
                .as_slice()
                .try_into()
                .map_err(|_| Error::Corrupted("room member"))?;
            let record = member_record_id(room, &device);
            cards.push(self.read_record("room-member", &record, &nonce, &ciphertext)?);
        }
        Ok(cards)
    }
}

pub(super) fn insert_welcome(tx: &Transaction<'_>, welcome: &WelcomeStore) -> Result<()> {
    insert_room(
        tx,
        &welcome.conversation,
        welcome.kind,
        &welcome.title,
        &welcome.owner,
        welcome.created_at_ms,
        &welcome.members,
    )
}

pub(super) fn flush_pending(
    tx: &Transaction<'_>,
    room: &ConversationId,
    devices: &[[u8; 32]],
) -> Result<Vec<MessageId>> {
    let allowed: HashSet<[u8; 32]> = devices.iter().copied().collect();
    let mut statement = tx.prepare(
        "SELECT message_id, author_account, author_device, created_at_ms, body_nonce, body_ciphertext \
         FROM pending_room_messages WHERE room_id = ?1",
    )?;
    let rows = statement.query_map([room.as_bytes().as_slice()], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Vec<u8>>(4)?,
            row.get::<_, Vec<u8>>(5)?,
        ))
    })?;
    let pending = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);
    let mut inserted = Vec::new();
    for (message_id, account, device, created, nonce, ciphertext) in pending {
        let id = MessageId::from_slice(&message_id).map_err(|_| Error::Corrupted("pending room message"))?;
        let author_account = AccountId::from_slice(&account).map_err(|_| Error::Corrupted("pending room author"))?;
        let author_device = DeviceId::from_slice(&device).map_err(|_| Error::Corrupted("pending room author"))?;
        let device_bytes: [u8; 32] = device
            .as_slice()
            .try_into()
            .map_err(|_| Error::Corrupted("pending room author"))?;
        if allowed.contains(&device_bytes) {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM messages WHERE id = ?1)",
                [id.as_bytes().as_slice()],
                |row| row.get(0),
            )?;
            if !exists {
                let nonce: [u8; NONCE_LEN] = nonce
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::Corrupted("pending room message"))?;
                delivery::insert_message(
                    tx,
                    &id,
                    room,
                    &author_account,
                    &author_device,
                    created,
                    &SealedBody { nonce, ciphertext },
                    MessageState::Received,
                )?;
                inserted.push(id);
            }
            apply_held_room_edit(tx, &id, &author_account, &author_device)?;
        }
        tx.execute(
            "DELETE FROM pending_room_messages WHERE message_id = ?1",
            [id.as_bytes().as_slice()],
        )?;
    }
    let mut ops = tx.prepare("SELECT message_id, author_device FROM pending_room_ops WHERE room_id = ?1")?;
    let stale = ops
        .query_map([room.as_bytes().as_slice()], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(ops);
    for (message_id, device) in stale {
        let device_bytes: [u8; 32] = match device.as_slice().try_into() {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if !allowed.contains(&device_bytes) {
            tx.execute(
                "DELETE FROM pending_room_ops WHERE message_id = ?1 AND author_device = ?2",
                params![message_id, device],
            )?;
        }
    }
    Ok(inserted)
}

pub(super) fn insert_held_text(tx: &Transaction<'_>, held: &HeldRoomText) -> Result<()> {
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pending_room_messages WHERE message_id = ?1)",
        [held.id.as_bytes().as_slice()],
        |row| row.get(0),
    )?;
    if exists {
        return Ok(());
    }
    let count: i64 = tx.query_row("SELECT count(*) FROM pending_room_messages", [], |row| row.get(0))?;
    if count >= MAX_HELD {
        return Err(Error::Network("room is not ready"));
    }
    tx.execute(
        "INSERT INTO pending_room_messages \
            (message_id, room_id, author_account, author_device, created_at_ms, body_nonce, body_ciphertext) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            held.id.as_bytes().as_slice(),
            held.room_id.as_bytes().as_slice(),
            held.author_account.as_bytes().as_slice(),
            held.author_device.as_bytes().as_slice(),
            held.created_at_ms,
            held.sealed.nonce.as_slice(),
            held.sealed.ciphertext,
        ],
    )?;
    Ok(())
}

pub(super) fn insert_held_edit(tx: &Transaction<'_>, held: &HeldRoomEdit) -> Result<()> {
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pending_room_ops WHERE message_id = ?1 AND author_device = ?2)",
        params![held.id.as_bytes().as_slice(), held.author_device.as_bytes().as_slice()],
        |row| row.get(0),
    )?;
    if !exists {
        let count: i64 = tx.query_row("SELECT count(*) FROM pending_room_ops", [], |row| row.get(0))?;
        if count >= MAX_HELD {
            return Err(Error::Network("room is not ready"));
        }
    }
    tx.execute(
        "INSERT INTO pending_room_ops \
            (message_id, author_device, room_id, author_account, revision, deleted, edited_at_ms, body_nonce, body_ciphertext) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(message_id, author_device) DO UPDATE SET \
            revision = excluded.revision, \
            deleted = excluded.deleted, \
            edited_at_ms = excluded.edited_at_ms, \
            body_nonce = excluded.body_nonce, \
            body_ciphertext = excluded.body_ciphertext \
         WHERE excluded.revision > pending_room_ops.revision",
        params![
            held.id.as_bytes().as_slice(),
            held.author_device.as_bytes().as_slice(),
            held.room_id.as_bytes().as_slice(),
            held.author_account.as_bytes().as_slice(),
            i64::from(held.revision),
            i64::from(held.deleted),
            held.edited_at_ms,
            held.sealed.nonce.as_slice(),
            held.sealed.ciphertext,
        ],
    )?;
    Ok(())
}

pub(super) fn apply_held_room_edit(
    tx: &Transaction<'_>,
    id: &MessageId,
    account: &AccountId,
    device: &DeviceId,
) -> Result<()> {
    let row = tx
        .query_row(
            "SELECT revision, deleted, edited_at_ms, body_nonce, body_ciphertext, author_account \
             FROM pending_room_ops WHERE message_id = ?1 AND author_device = ?2",
            params![id.as_bytes().as_slice(), device.as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                ))
            },
        )
        .optional()?;
    if let Some((revision, deleted, edited_at, nonce, ciphertext, author_account)) = row
        && author_account.as_slice() == account.as_bytes()
    {
        tx.execute(
            "UPDATE messages SET body_nonce = ?1, body_ciphertext = ?2, revision = ?3, edited_at_ms = ?4, deleted = ?5 \
             WHERE id = ?6 AND author_account = ?7 AND author_device = ?8 AND deleted = 0 AND revision < ?3",
            params![
                nonce,
                ciphertext,
                revision,
                edited_at,
                deleted,
                id.as_bytes().as_slice(),
                account.as_bytes().as_slice(),
                device.as_bytes().as_slice(),
            ],
        )?;
    }
    tx.execute(
        "DELETE FROM pending_room_ops WHERE message_id = ?1 AND author_device = ?2",
        params![id.as_bytes().as_slice(), device.as_bytes().as_slice()],
    )?;
    Ok(())
}

pub(super) fn note_receipt(
    tx: &Transaction<'_>,
    message: &MessageId,
    sender: &[u8; 32],
    author: &[u8; 32],
) -> Result<bool> {
    tx.execute(
        "INSERT OR IGNORE INTO room_receipts (message_id, device_id) VALUES (?1, ?2)",
        params![message.as_bytes().as_slice(), sender],
    )?;
    let members: i64 = tx.query_row(
        "SELECT count(*) FROM room_members WHERE conversation_id = (SELECT conversation_id FROM messages WHERE id = ?1)",
        [message.as_bytes().as_slice()],
        |row| row.get(0),
    )?;
    let receipts: i64 = tx.query_row(
        "SELECT count(*) FROM room_receipts WHERE message_id = ?1",
        [message.as_bytes().as_slice()],
        |row| row.get(0),
    )?;
    if members >= 2 && receipts >= members - 1 {
        let changed = tx.execute(
            "UPDATE messages SET state = 'delivered' WHERE id = ?1 AND author_device = ?2 AND state IN ('queued', 'mailbox')",
            params![message.as_bytes().as_slice(), author],
        )?;
        return Ok(changed > 0);
    }
    Ok(false)
}

fn member_card<'a>(room: &'a RoomRecord, sender: &DeviceIdentity) -> Result<&'a ContactCard> {
    room.members
        .iter()
        .find(|card| card.identity == *sender)
        .ok_or_else(|| Error::InvalidArgument("sender is not a member of this room".into()))
}

fn same_membership(stored: &[ContactCard], incoming: &[ContactCard]) -> bool {
    stored.len() == incoming.len()
        && incoming.iter().all(|card| {
            stored
                .iter()
                .any(|old| old.identity == card.identity && old.inbox_key == card.inbox_key)
        })
}

fn protocol_kind(kind: ConversationKind) -> Result<RoomKind> {
    match kind {
        ConversationKind::Group => Ok(RoomKind::Group),
        ConversationKind::Channel => Ok(RoomKind::Channel),
        ConversationKind::SavedMessages | ConversationKind::Direct => Err(Error::InvalidArgument("not a room".into())),
    }
}

fn local_kind(kind: RoomKind) -> ConversationKind {
    match kind {
        RoomKind::Group => ConversationKind::Group,
        RoomKind::Channel => ConversationKind::Channel,
    }
}

fn member_record_id(room: &ConversationId, device: &[u8; 32]) -> [u8; 48] {
    let mut id = [0u8; 48];
    id[..16].copy_from_slice(room.as_bytes());
    id[16..].copy_from_slice(device);
    id
}

fn seal_members(store: &Store, room: &ConversationId, cards: &[ContactCard]) -> Result<Vec<StoredMember>> {
    cards
        .iter()
        .map(|card| {
            let record = member_record_id(room, &card.identity.device);
            Ok(StoredMember {
                device: card.identity.device,
                sealed: store.seal_record("room-member", &record, card)?,
            })
        })
        .collect()
}

fn seal_fanout(
    store: &Store,
    identity: &LocalIdentity,
    members: &[ContactCard],
    now: i64,
    payload: &Payload,
) -> Result<Vec<(MessageId, Vec<u8>, SealedBody)>> {
    let mut jobs = Vec::new();
    for card in members {
        if card.identity.device == *store.device_id.as_bytes() {
            continue;
        }
        let id = MessageId::random()?;
        jobs.push((
            id,
            delivery::seal(identity, card, now, payload)?,
            store.seal_record("route", id.as_bytes(), card)?,
        ));
    }
    if jobs.is_empty() {
        return Err(Error::InvalidArgument("room has no other members".into()));
    }
    Ok(jobs)
}

fn insert_room(
    tx: &Transaction<'_>,
    id: &ConversationId,
    kind: ConversationKind,
    title: &str,
    owner: &[u8; 32],
    now: i64,
    members: &[StoredMember],
) -> Result<()> {
    tx.execute(
        "INSERT INTO conversations (id, kind, created_at_ms) VALUES (?1, ?2, ?3)",
        params![id.as_bytes().as_slice(), kind.as_str(), now],
    )?;
    tx.execute(
        "INSERT INTO rooms (conversation_id, title, owner_device) VALUES (?1, ?2, ?3)",
        params![id.as_bytes().as_slice(), title, owner],
    )?;
    for member in members {
        tx.execute(
            "INSERT INTO room_members (conversation_id, device_id, nonce, ciphertext) VALUES (?1, ?2, ?3, ?4)",
            params![
                id.as_bytes().as_slice(),
                member.device.as_slice(),
                member.sealed.nonce.as_slice(),
                member.sealed.ciphertext,
            ],
        )?;
    }
    Ok(())
}
