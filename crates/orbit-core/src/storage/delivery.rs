//! Single-device contact exchange and transactional mailbox delivery.
//! Secrets in contact cards/config/routes are sealed under the local device key.

use orbit_protocol::NodeAddress;
use orbit_protocol::envelope::{self, ContactCard, Invite, Payload};
use orbit_protocol::mailbox::{DepositToken, Item, ItemId, MailboxId};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::cipher::SealedBody;
use super::voice;
use super::{Store, StoredMessage};
use crate::domain::{
    AccountId, Contact, ConversationId, ConversationKind, DeviceId, InvitePreview, Message, MessageBody, MessageId,
    MessageState, normalize_profile, normalize_text,
};
use crate::error::{Error, Result};
use crate::identity::LocalIdentity;

const MAX_PENDING: i64 = 1000;
const INVITE_LIFETIME_MS: i64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DeliveryConfig {
    pub node: String,
    pub token: DepositToken,
    pub registered: bool,
}

impl DeliveryConfig {
    /// Bare endpoint id: the device is reachable without a mailbox node.
    pub(crate) fn is_direct(&self) -> bool {
        self.node
            .parse::<NodeAddress>()
            .is_ok_and(|address| address.addrs.is_empty())
    }
}

#[derive(Debug)]
pub(crate) struct OutboxItem {
    pub id: MessageId,
    pub route: ContactCard,
    pub envelope: Vec<u8>,
}

#[derive(Debug, Default)]
pub(crate) struct InboxChanges {
    pub messages: Vec<Message>,
    pub contacts_changed: bool,
}

impl Store {
    pub(crate) fn delivery_config(&self) -> Result<Option<DeliveryConfig>> {
        let row = self
            .conn
            .query_row("SELECT nonce, ciphertext FROM delivery_config WHERE id = 1", [], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .optional()?;
        row.map(|(nonce, ciphertext)| self.read_record("delivery_config", b"1", &nonce, &ciphertext))
            .transpose()
    }

    pub(crate) fn configure_delivery(&mut self, node: &str) -> Result<DeliveryConfig> {
        if node.len() > orbit_protocol::envelope::MAX_CARD_NODE_BYTES {
            return Err(Error::InvalidArgument("node address is too long".into()));
        }
        let node: NodeAddress = node
            .parse()
            .map_err(|_| Error::InvalidArgument("invalid node address".into()))?;
        if node.addrs.is_empty() {
            return Err(Error::InvalidArgument("node address needs a reachable socket".into()));
        }
        let node = node.to_string();
        let config = match self.delivery_config()? {
            Some(mut old) => {
                if old.node != node {
                    let used: i64 = self.conn.query_row(
                        "SELECT (SELECT count(*) FROM contacts) + (SELECT count(*) FROM invitations)",
                        [],
                        |r| r.get(0),
                    )?;
                    if used > 0 {
                        return Err(Error::InvalidArgument(
                            "cannot change node after sharing invitations or adding contacts".into(),
                        ));
                    }
                    old.node = node;
                    old.registered = false;
                }
                old
            }
            None => DeliveryConfig {
                node,
                token: DepositToken::generate().map_err(|_| Error::Random)?,
                registered: false,
            },
        };
        self.save_delivery_config(&config)?;
        Ok(config)
    }

    /// Starts direct delivery. A bare endpoint id needs no registration code.
    /// An existing mailbox configuration is left as it is.
    pub(crate) fn configure_direct(&mut self, endpoint_id: [u8; 32]) -> Result<DeliveryConfig> {
        if let Some(config) = self.delivery_config()? {
            return Ok(config);
        }
        let config = DeliveryConfig {
            node: hex::encode(endpoint_id),
            token: DepositToken::generate().map_err(|_| Error::Random)?,
            registered: true,
        };
        self.save_delivery_config(&config)?;
        Ok(config)
    }

    pub(crate) fn mark_registered(&mut self, node: &str) -> Result<()> {
        let mut config = self.delivery_config()?.ok_or(Error::NetworkNotConfigured)?;
        if config.node != node {
            return Err(Error::Internal("registration node changed"));
        }
        config.registered = true;
        self.save_delivery_config(&config)
    }

    fn save_delivery_config(&self, config: &DeliveryConfig) -> Result<()> {
        let sealed = self.seal_record("delivery_config", b"1", config)?;
        self.conn.execute(
            "INSERT INTO delivery_config (id, nonce, ciphertext) VALUES (1, ?1, ?2)
            ON CONFLICT(id) DO UPDATE SET nonce=excluded.nonce, ciphertext=excluded.ciphertext",
            params![sealed.nonce.as_slice(), sealed.ciphertext],
        )?;
        Ok(())
    }

    pub(crate) fn own_card(&self, identity: &LocalIdentity) -> Result<ContactCard> {
        let config = self
            .delivery_config()?
            .filter(|c| c.registered)
            .ok_or(Error::NetworkNotConfigured)?;
        let profile = self.profile()?.ok_or(Error::InvalidArgument(
            "set a profile before sharing an invitation".into(),
        ))?;
        Ok(ContactCard {
            identity: identity.public().to_device_identity(),
            inbox_key: identity.inbox().public(),
            node: config.node,
            mailbox: MailboxId(identity.mailbox_key().verifying_key().to_bytes()),
            token: config.token,
            display_name: profile.display_name,
        })
    }

    pub(crate) fn create_invite(&mut self, identity: &LocalIdentity, now: i64) -> Result<String> {
        let invite_id = *MessageId::random()?.as_bytes();
        let expires_at_ms = now.saturating_add(INVITE_LIFETIME_MS);
        let invite = Invite::create(
            identity.device_key(),
            invite_id,
            expires_at_ms,
            self.own_card(identity)?,
        )
        .map_err(|_| Error::InvalidInvite("cannot create invitation"))?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM invitations WHERE expires_at_ms <= ?1", [now])?;
        let count: i64 = tx.query_row("SELECT count(*) FROM invitations", [], |r| r.get(0))?;
        if count >= 100 {
            return Err(Error::Busy);
        }
        tx.execute(
            "INSERT INTO invitations (id, expires_at_ms) VALUES (?1, ?2)",
            params![invite_id.as_slice(), expires_at_ms],
        )?;
        tx.commit()?;
        Ok(invite.to_text())
    }

    pub(crate) fn inspect_invite(&self, text: &str, now: i64) -> Result<InvitePreview> {
        let invite = self.checked_invite(text, now)?;
        Ok(InvitePreview {
            account_id: AccountId::from_bytes(invite.card.identity.account),
            device_id: DeviceId::from_bytes(invite.card.identity.device),
            display_name: invite.card.display_name,
            expires_at_ms: invite.expires_at_ms,
        })
    }

    fn checked_invite(&self, text: &str, now: i64) -> Result<Invite> {
        let invite = Invite::parse(text, now)
            .map_err(|_| Error::InvalidInvite("malformed, expired or unverified invitation"))?;
        self.check_card(&invite.card)?;
        Ok(invite)
    }

    fn check_card(&self, card: &ContactCard) -> Result<()> {
        card.node
            .parse::<NodeAddress>()
            .map_err(|_| Error::InvalidInvite("invalid node address"))?;
        card.identity
            .verify()
            .map_err(|_| Error::InvalidInvite("invalid device certificate"))?;
        normalize_profile(&card.display_name, "").map_err(|_| Error::InvalidInvite("invalid profile name"))?;
        if card.identity.account == *self.account_id.as_bytes() {
            return Err(Error::InvalidInvite("this invitation belongs to your own account"));
        }
        Ok(())
    }

    fn direct_id(&self, device: &[u8; 32]) -> ConversationId {
        let own = self.device_id.as_bytes();
        let (first, second) = if own < device { (own, device) } else { (device, own) };
        let mut bytes = b"orbit/v1/direct-conversation\0".to_vec();
        bytes.extend_from_slice(first);
        bytes.extend_from_slice(second);
        let hash = blake3::hash(&bytes);
        let mut id = [0u8; 16];
        id.copy_from_slice(&hash.as_bytes()[..16]);
        ConversationId::from_bytes(id)
    }

    pub(crate) fn contact(&self, id: &ConversationId) -> Result<Option<Contact>> {
        self.contact_card(id)?
            .map(|(card, ready)| {
                Ok(Contact {
                    conversation_id: *id,
                    account_id: AccountId::from_bytes(card.identity.account),
                    device_id: DeviceId::from_bytes(card.identity.device),
                    display_name: card.display_name,
                    ready,
                    avatar: self.contact_avatar(id)?,
                })
            })
            .transpose()
    }

    pub(super) fn contact_card(&self, id: &ConversationId) -> Result<Option<(ContactCard, bool)>> {
        let row = self
            .conn
            .query_row(
                "SELECT nonce, ciphertext, ready FROM contacts WHERE conversation_id = ?1",
                [id.as_bytes().as_slice()],
                |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, bool>(2)?)),
            )
            .optional()?;
        row.map(|(nonce, ciphertext, ready)| {
            self.read_record("contact", id.as_bytes(), &nonce, &ciphertext)
                .map(|card| (card, ready))
        })
        .transpose()
    }

    fn contact_avatar(&self, id: &ConversationId) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn
            .query_row(
                "SELECT avatar FROM contacts WHERE conversation_id=?1",
                [id.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Stores a JPEG or PNG, or clears it. Ready contacts receive a copy.
    pub(crate) fn set_avatar(
        &mut self,
        identity: &LocalIdentity,
        image: Option<Vec<u8>>,
        now: i64,
    ) -> Result<crate::domain::Profile> {
        if self.profile()?.is_none() {
            return Err(Error::NotFound("profile"));
        }
        if let Some(bytes) = &image
            && !orbit_protocol::envelope::valid_avatar(bytes)
        {
            return Err(Error::InvalidArgument("avatar".into()));
        }
        let ready = self.ready_cards()?;
        let payload_image = image.clone().unwrap_or_default();
        let mut jobs = Vec::with_capacity(ready.len());
        for (id, card) in &ready {
            let job = MessageId::random()?;
            let envelope = seal(
                identity,
                card,
                now,
                &Payload::Avatar {
                    image: payload_image.clone(),
                },
            )?;
            let route = self.seal_record("route", job.as_bytes(), card)?;
            jobs.push((job, *id, envelope, route));
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let blob: Option<&[u8]> = image.as_deref();
        let changed = tx.execute("UPDATE profile SET avatar=?1 WHERE id=1", params![blob])?;
        if changed == 0 {
            return Err(Error::NotFound("profile"));
        }
        for (job, id, envelope, route) in &jobs {
            insert_outbox(&tx, job, id, None, envelope, route)?;
        }
        tx.commit()?;
        self.profile()?.ok_or(Error::Internal("profile"))
    }

    fn ready_cards(&self) -> Result<Vec<(ConversationId, ContactCard)>> {
        let mut statement = self
            .conn
            .prepare("SELECT conversation_id, nonce, ciphertext FROM contacts WHERE ready=1")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id_bytes, nonce, ciphertext) = row?;
            let id = ConversationId::from_slice(&id_bytes).map_err(|_| Error::Corrupted("contact id"))?;
            out.push((id, self.read_record("contact", id.as_bytes(), &nonce, &ciphertext)?));
        }
        Ok(out)
    }

    pub(crate) fn accept_invite(&mut self, identity: &LocalIdentity, text: &str, now: i64) -> Result<Contact> {
        let invite = self.checked_invite(text, now)?;
        let id = self.direct_id(&invite.card.identity.device);
        if let Some((old, _)) = self.contact_card(&id)? {
            if old.identity != invite.card.identity || old.inbox_key != invite.card.inbox_key {
                return Err(Error::InvalidInvite("contact keys have changed"));
            }
            return self.contact(&id)?.ok_or(Error::NotFound("contact"));
        }
        let own_card = self.own_card(identity)?;
        let envelope = seal(
            identity,
            &invite.card,
            now,
            &Payload::ContactRequest {
                invite_id: invite.invite_id,
                card: own_card,
            },
        )?;
        let card = self.seal_record("contact", id.as_bytes(), &invite.card)?;
        let job = MessageId::random()?;
        let route = self.seal_record("route", job.as_bytes(), &invite.card)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_contact(&tx, &id, &invite.card, &card, false, now)?;
        insert_outbox(&tx, &job, &id, None, &envelope, &route)?;
        tx.commit()?;
        self.contact(&id)?.ok_or(Error::NotFound("contact"))
    }

    pub(crate) fn queue_text(
        &mut self,
        identity: &LocalIdentity,
        conversation: &ConversationId,
        text: String,
        now: i64,
    ) -> Result<Message> {
        let (card, _) = self.contact_card(conversation)?.ok_or(Error::NotFound("contact"))?;
        let id = MessageId::random()?;
        let envelope = seal(
            identity,
            &card,
            now,
            &Payload::Text {
                message_id: *id.as_bytes(),
                text: text.clone(),
            },
        )?;
        let body = MessageBody::Text { text };
        let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&body).map_err(|_| Error::Internal("body encoding"))?);
        let sealed = self.cipher.seal(&id, conversation, &bytes)?;
        let route = self.seal_record("route", id.as_bytes(), &card)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_message(
            &tx,
            &id,
            conversation,
            &self.account_id,
            &self.device_id,
            now,
            &sealed,
            MessageState::Queued,
        )?;
        let seq = tx.last_insert_rowid() as u64;
        insert_outbox(&tx, &id, conversation, Some(&id), &envelope, &route)?;
        tx.commit()?;
        Ok(Message {
            id,
            conversation_id: *conversation,
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

    /// Edits or deletes a message this device authored. `text == None` deletes.
    /// A direct change is sealed into the outbox; saved messages stay local.
    pub(crate) fn revise_own(
        &mut self,
        identity: &LocalIdentity,
        conversation: &ConversationId,
        message_id: &MessageId,
        text: Option<String>,
        now: i64,
    ) -> Result<Message> {
        let current = self.message(message_id)?;
        if current.conversation_id != *conversation {
            return Err(Error::NotFound("message"));
        }
        if current.author_device != self.device_id || current.author_account != self.account_id {
            return Err(Error::InvalidArgument("only the author can change this message".into()));
        }
        if current.deleted {
            return if text.is_none() {
                Ok(current)
            } else {
                Err(Error::InvalidArgument("message is deleted".into()))
            };
        }
        if text.is_some() && !matches!(current.body, MessageBody::Text { .. }) {
            return Err(Error::InvalidArgument("only a text message can be edited".into()));
        }
        if let (Some(next), MessageBody::Text { text: previous }) = (&text, &current.body)
            && next == previous
        {
            return Ok(current);
        }
        let revision = current
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::InvalidArgument("message can no longer be changed".into()))?;
        let deleted = text.is_none();
        let body = match &text {
            Some(value) => MessageBody::Text { text: value.clone() },
            None => MessageBody::Deleted,
        };
        let sealed = seal_body(self, message_id, conversation, &body)?;
        let kind = self.conversation(conversation)?.kind;
        let fanout = match kind {
            ConversationKind::Direct => {
                let (card, _) = self.contact_card(conversation)?.ok_or(Error::NotFound("contact"))?;
                let payload = match &text {
                    Some(value) => Payload::EditText {
                        message_id: *message_id.as_bytes(),
                        revision,
                        text: value.clone(),
                    },
                    None => Payload::DeleteText {
                        message_id: *message_id.as_bytes(),
                        revision,
                    },
                };
                let job = MessageId::random()?;
                vec![(
                    job,
                    seal(identity, &card, now, &payload)?,
                    self.seal_record("route", job.as_bytes(), &card)?,
                )]
            }
            ConversationKind::Group | ConversationKind::Channel => {
                self.room_revision_envelopes(identity, conversation, message_id, revision, text.as_deref(), now)?
            }
            ConversationKind::SavedMessages => Vec::new(),
        };
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = apply_revision(
            &tx,
            &RevisionWrite {
                id: *message_id,
                sealed,
                revision,
                edited_at_ms: now,
                deleted,
                author_account: self.account_id,
                author_device: self.device_id,
            },
        )?;
        if !updated {
            return Err(Error::InvalidArgument("message can no longer be changed".into()));
        }
        if deleted {
            tx.execute(
                "DELETE FROM voice_notes WHERE message_id=?1",
                [message_id.as_bytes().as_slice()],
            )?;
        }
        for (job, envelope, route) in &fanout {
            // The outbox id differs from the message id, so depositing an edit does not
            // mark the original text as stored.
            insert_outbox(&tx, job, conversation, Some(message_id), envelope, route)?;
        }
        tx.commit()?;
        self.message(message_id)
    }

    /// Sends text only after the mutually authenticated contact exchange. Handshake
    /// and receipt envelopes are not held back by a pending contact.
    pub(crate) fn outbox(&self, limit: u32) -> Result<Vec<OutboxItem>> {
        let mut statement = self.conn.prepare(
            "SELECT o.id, o.envelope, o.route_nonce, o.route_ciphertext FROM outbox o
            LEFT JOIN contacts c ON c.conversation_id = o.conversation_id
            LEFT JOIN rooms r ON r.conversation_id = o.conversation_id
            WHERE o.message_id IS NULL OR c.ready = 1 OR r.conversation_id IS NOT NULL
            ORDER BY o.rowid LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (id, envelope, nonce, ciphertext) = row?;
            let id = MessageId::from_slice(&id).map_err(|_| Error::Corrupted("outbox id"))?;
            Ok(OutboxItem {
                id,
                envelope,
                route: self.read_record("route", id.as_bytes(), &nonce, &ciphertext)?,
            })
        })
        .collect()
    }

    pub(crate) fn deposited(&mut self, id: &MessageId) -> Result<Option<Message>> {
        let message_id: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT message_id FROM outbox WHERE id=?1",
                [id.as_bytes().as_slice()],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Only the original text envelope uses the message id as its outbox id.
        // Edits share the message id and must not mark that text as stored.
        if let Some(ref message_id) = message_id
            && message_id.as_slice() == id.as_bytes()
        {
            tx.execute(
                "UPDATE messages SET state='mailbox' WHERE id=?1 AND state='queued'",
                [message_id],
            )?;
        }
        tx.execute("DELETE FROM outbox WHERE id=?1", [id.as_bytes().as_slice()])?;
        if let Some(ref message_id) = message_id {
            tx.execute(
                "UPDATE messages SET state='mailbox' WHERE id=?1 AND state='queued' \
                 AND NOT EXISTS (SELECT 1 FROM outbox WHERE message_id=?1)",
                [message_id],
            )?;
        }
        tx.commit()?;
        message_id
            .map(|id| self.message(&MessageId::from_slice(&id).map_err(|_| Error::Corrupted("outbox message id"))?))
            .transpose()
    }

    fn message(&self, id: &MessageId) -> Result<Message> {
        self.lookup_message(id)?.ok_or(Error::NotFound("message"))
    }

    pub(super) fn lookup_message(&self, id: &MessageId) -> Result<Option<Message>> {
        let sql = format!("SELECT {} FROM messages WHERE id=?1", super::MESSAGE_COLUMNS);
        let row = self
            .conn
            .query_row(&sql, [id.as_bytes().as_slice()], StoredMessage::from_row)
            .optional()?;
        row.map(|row| self.decode(row)).transpose()
    }

    pub(crate) fn reject_item(&mut self, id: &ItemId) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO processed_inbox (id,accepted) VALUES (?1,0)",
            [id.0.as_slice()],
        )?;
        Ok(())
    }

    pub(crate) fn receive_item(&mut self, identity: &LocalIdentity, item: &Item, now: i64) -> Result<InboxChanges> {
        if ItemId::of(&item.envelope) != item.id {
            return Err(Error::InvalidArgument("envelope content address mismatch".into()));
        }
        let rejected: Option<bool> = self
            .conn
            .query_row(
                "SELECT accepted FROM processed_inbox WHERE id=?1",
                [item.id.0.as_slice()],
                |r| r.get(0),
            )
            .optional()?;
        if rejected == Some(false) {
            return Ok(InboxChanges::default());
        }
        let opened = envelope::open(identity.inbox(), identity.public().device_id.as_bytes(), &item.envelope)
            .map_err(|_| Error::InvalidArgument("invalid incoming envelope".into()))?;
        let conversation = self.direct_id(&opened.sender.device);
        let existing = self.contact_card(&conversation)?;
        let mut card_change = None;
        let mut received = None;
        let mut delivered = Vec::new();
        let mut reply = None;
        let mut revision_plan = None;
        let mut message_conversation = None;
        let mut reply_conversation = None;
        let mut welcome = None;
        let mut hold_text = None;
        let mut hold_edit = None;
        let mut room_receipts = Vec::new();
        let mut avatar_update = None;
        let mut voice_effect = None;
        match opened.payload {
            Payload::ContactRequest { invite_id, card } => {
                self.check_card(&card)?;
                let valid: bool = self.conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM invitations WHERE id=?1 AND expires_at_ms>?2)",
                    params![invite_id.as_slice(), now],
                    |r| r.get(0),
                )?;
                if !valid {
                    return Err(Error::InvalidArgument("unknown or expired invitation".into()));
                }
                check_pinned(existing.as_ref(), &card)?;
                reply = Some((
                    card.clone(),
                    Payload::CardUpdate {
                        card: self.own_card(identity)?,
                    },
                ));
                card_change = Some(card);
            }
            Payload::CardUpdate { card } => {
                self.check_card(&card)?;
                if existing.is_none() {
                    return Err(Error::NotFound("contact"));
                }
                check_pinned(existing.as_ref(), &card)?;
                card_change = Some(card);
            }
            Payload::Text { message_id, text } => {
                let (card, _) = existing.as_ref().ok_or(Error::NotFound("contact"))?;
                if card.identity != opened.sender {
                    return Err(Error::InvalidArgument("sender is not the pinned contact".into()));
                }
                let id = MessageId::from_bytes(message_id);
                let text = normalize_text(&text)?;
                if let Some(old) = self.lookup_message(&id)? {
                    let same_author = old.conversation_id == conversation
                        && old.author_device.as_bytes() == &opened.sender.device
                        && old.author_account.as_bytes() == &opened.sender.account;
                    let same_text = old.body == (MessageBody::Text { text: text.clone() });
                    // A later edit keeps the id. Replaying the original text must not quarantine it.
                    if !same_author || (!same_text && old.revision == 0 && !old.deleted) {
                        return Err(Error::InvalidArgument("conflicting message id".into()));
                    }
                } else {
                    let body = MessageBody::Text { text };
                    let bytes = zeroize::Zeroizing::new(
                        serde_json::to_vec(&body).map_err(|_| Error::Internal("body encoding"))?,
                    );
                    received = Some((id, self.cipher.seal(&id, &conversation, &bytes)?));
                }
                // Also reissue a receipt for duplicates after a lost receipt/deposit ACK.
                reply = Some((
                    card.clone(),
                    Payload::Delivered {
                        message_ids: vec![message_id],
                    },
                ));
            }
            Payload::Delivered { message_ids } => {
                let ids = message_ids.into_iter().map(MessageId::from_bytes).collect::<Vec<_>>();
                match &existing {
                    Some((card, _)) if card.identity == opened.sender => delivered = ids.clone(),
                    Some(_) => {
                        return Err(Error::InvalidArgument("receipt is not from the pinned contact".into()));
                    }
                    None => {}
                }
                room_receipts = self.room_receipts_for(&opened.sender, &ids)?;
                if delivered.is_empty() && room_receipts.is_empty() {
                    return Err(Error::InvalidArgument("receipt is not from a member".into()));
                }
            }
            Payload::RoomWelcome {
                room_id,
                kind,
                title,
                members,
            } => {
                welcome = self.plan_welcome(
                    identity,
                    &opened.sender,
                    super::rooms::IncomingWelcome {
                        room_id,
                        kind,
                        title,
                        members,
                    },
                    now,
                )?;
            }
            Payload::RoomText {
                room_id,
                message_id,
                text,
            } => match self.plan_room_text(&opened.sender, opened.sent_at_ms, room_id, message_id, &text)? {
                super::rooms::RoomTextPlan::Incoming(incoming) => {
                    let incoming = *incoming;
                    message_conversation = Some(incoming.conversation);
                    if let Some(sealed) = incoming.sealed {
                        received = Some((incoming.id, sealed));
                    }
                    if let Some(reply_to) = incoming.reply {
                        reply_conversation = Some(incoming.conversation);
                        reply = Some(reply_to);
                    }
                }
                super::rooms::RoomTextPlan::Hold(held) => hold_text = Some(held),
            },
            Payload::RoomEditText {
                room_id,
                message_id,
                revision,
                text,
            } => {
                message_conversation = Some(ConversationId::from_bytes(room_id));
                match self.plan_room_revision(
                    &opened.sender,
                    opened.sent_at_ms,
                    room_id,
                    message_id,
                    revision,
                    Some(text),
                )? {
                    super::rooms::RoomRevisionPlan::Now(plan) => revision_plan = plan,
                    super::rooms::RoomRevisionPlan::Later(held) => hold_edit = Some(held),
                }
            }
            Payload::RoomDeleteText {
                room_id,
                message_id,
                revision,
            } => {
                message_conversation = Some(ConversationId::from_bytes(room_id));
                match self.plan_room_revision(&opened.sender, opened.sent_at_ms, room_id, message_id, revision, None)? {
                    super::rooms::RoomRevisionPlan::Now(plan) => revision_plan = plan,
                    super::rooms::RoomRevisionPlan::Later(held) => hold_edit = Some(held),
                }
            }
            Payload::EditText {
                message_id,
                revision,
                text,
            } => {
                revision_plan = self.plan_revision(RevisionRequest {
                    conversation: &conversation,
                    existing: existing.as_ref(),
                    sender: &opened.sender,
                    message_id,
                    revision,
                    text: Some(text),
                    edited_at_ms: opened.sent_at_ms,
                })?;
            }
            Payload::DeleteText { message_id, revision } => {
                revision_plan = self.plan_revision(RevisionRequest {
                    conversation: &conversation,
                    existing: existing.as_ref(),
                    sender: &opened.sender,
                    message_id,
                    revision,
                    text: None,
                    edited_at_ms: opened.sent_at_ms,
                })?;
            }
            Payload::Avatar { image } => {
                let (card, _) = existing.as_ref().ok_or(Error::NotFound("contact"))?;
                if card.identity != opened.sender {
                    return Err(Error::InvalidArgument("sender is not the pinned contact".into()));
                }
                if !image.is_empty() && !orbit_protocol::envelope::valid_avatar(&image) {
                    return Err(Error::InvalidArgument("avatar".into()));
                }
                avatar_update = Some(image);
            }
            Payload::MediaStart {
                message_id,
                duration_ms,
                byte_len,
                sha256,
                chunk_count,
                waveform,
            } => {
                voice_effect = Some(self.plan_voice_start(
                    &opened.sender,
                    &conversation,
                    opened.sent_at_ms,
                    voice::VoiceStart {
                        message_id,
                        duration_ms,
                        byte_len,
                        sha256,
                        chunk_count,
                        waveform,
                    },
                )?);
            }
            Payload::MediaChunk {
                message_id,
                index,
                bytes,
            } => {
                voice_effect = Some(self.plan_voice_chunk(&opened.sender, &conversation, message_id, index, bytes)?);
            }
        }
        if let Some(effect) = &voice_effect
            && effect.receipt
            && let Some((card, _)) = existing.as_ref()
        {
            reply = Some((
                card.clone(),
                Payload::Delivered {
                    message_ids: vec![*effect.id.as_bytes()],
                },
            ));
        }
        let becoming_ready = card_change.is_some() && existing.as_ref().is_none_or(|(_, ready)| !ready);
        let avatar_share = if becoming_ready
            && let (Some(card), Some(profile)) = (card_change.as_ref(), self.profile()?)
            && let Some(image) = profile.avatar
        {
            let job = MessageId::random()?;
            let envelope = seal(identity, card, now, &Payload::Avatar { image })?;
            let route = self.seal_record("route", job.as_bytes(), card)?;
            Some((job, envelope, route))
        } else {
            None
        };
        let sealed_card = card_change
            .as_ref()
            .map(|c| self.seal_record("contact", conversation.as_bytes(), c))
            .transpose()?;
        let job_id = {
            let mut id = [0u8; 16];
            id.copy_from_slice(&item.id.0[..16]);
            MessageId::from_bytes(id)
        };
        let reply = reply
            .map(|(card, payload)| -> Result<_> {
                Ok((
                    seal(identity, &card, now, &payload)?,
                    self.seal_record("route", job_id.as_bytes(), &card)?,
                ))
            })
            .transpose()?;
        let voice_wav = if let Some(effect) = voice_effect.as_mut()
            && let Some(finish) = effect.finish.take()
        {
            received = Some((finish.id, finish.sealed));
            Some((finish.id, finish.wav))
        } else {
            None
        };
        let stored_in = message_conversation.unwrap_or(conversation);
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let (Some(card), Some(sealed)) = (&card_change, &sealed_card) {
            insert_contact(&tx, &conversation, card, sealed, true, now)?;
        }
        if let Some((job, envelope, route)) = &avatar_share {
            insert_outbox(&tx, job, &conversation, None, envelope, route)?;
        }
        let mut avatar_applied = false;
        if let Some(image) = &avatar_update {
            let blob: Option<&[u8]> = if image.is_empty() { None } else { Some(image.as_slice()) };
            let changed = tx.execute(
                "UPDATE contacts SET avatar=?1, avatar_updated_at_ms=?2 WHERE conversation_id=?3 \
                 AND (avatar_updated_at_ms IS NULL OR avatar_updated_at_ms < ?2)",
                params![blob, opened.sent_at_ms, conversation.as_bytes().as_slice()],
            )?;
            avatar_applied = changed > 0;
        }
        if let Some(effect) = &voice_effect {
            voice::write_effect(&tx, effect)?;
        }
        if let Some((id, wav)) = &voice_wav {
            voice::store_wav(&tx, id, wav)?;
            voice::clear_incoming(&tx, id)?;
        }
        let mut flushed = Vec::new();
        if let Some(welcome) = &welcome {
            super::rooms::insert_welcome(&tx, welcome)?;
            let devices: Vec<[u8; 32]> = welcome.members.iter().map(|member| member.device).collect();
            flushed = super::rooms::flush_pending(&tx, &welcome.conversation, &devices)?;
        }
        if let Some(held) = &hold_text {
            super::rooms::insert_held_text(&tx, held)?;
        }
        if let Some(held) = &hold_edit {
            super::rooms::insert_held_edit(&tx, held)?;
        }
        let author_account = AccountId::from_bytes(opened.sender.account);
        let author_device = DeviceId::from_bytes(opened.sender.device);
        let received_id = if let Some((id, sealed)) = received {
            insert_message(
                &tx,
                &id,
                &stored_in,
                &author_account,
                &author_device,
                opened.sent_at_ms,
                &sealed,
                MessageState::Received,
            )?;
            Some(id)
        } else {
            None
        };
        if let Some(id) = &received_id {
            promote_pending(&tx, id, &author_account, &author_device)?;
            super::rooms::apply_held_room_edit(&tx, id, &author_account, &author_device)?;
            voice::forget_if_deleted(&tx, id)?;
        }
        let mut changed_ids = Vec::new();
        if let Some(plan) = revision_plan {
            match plan {
                IncomingRevision::Apply(write) => {
                    if apply_revision(&tx, &write)? {
                        changed_ids.push(write.id);
                        voice::forget_if_deleted(&tx, &write.id)?;
                    }
                }
                IncomingRevision::Hold(write) => hold_revision(&tx, &stored_in, &write)?,
            }
        }
        for id in delivered {
            let changed = tx.execute("UPDATE messages SET state='delivered' WHERE id=?1 AND conversation_id=?2 AND author_device=?3 AND state IN ('queued','mailbox')",
                params![id.as_bytes().as_slice(), conversation.as_bytes().as_slice(), self.device_id.as_bytes().as_slice()])?;
            if changed > 0 {
                changed_ids.push(id);
            }
        }
        for id in &room_receipts {
            if super::rooms::note_receipt(&tx, id, &opened.sender.device, self.device_id.as_bytes())? {
                changed_ids.push(*id);
            }
        }
        if let Some((envelope, route)) = reply {
            let reply_in = reply_conversation.unwrap_or(conversation);
            insert_outbox(&tx, &job_id, &reply_in, None, &envelope, &route)?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO processed_inbox (id,accepted) VALUES (?1,1)",
            [item.id.0.as_slice()],
        )?;
        tx.commit()?;
        if let Some(id) = received_id {
            changed_ids.push(id);
        }
        changed_ids.extend(flushed);
        Ok(InboxChanges {
            messages: changed_ids.iter().map(|id| self.message(id)).collect::<Result<_>>()?,
            contacts_changed: card_change.is_some() || welcome.is_some() || avatar_applied,
        })
    }

    fn plan_revision(&self, request: RevisionRequest<'_>) -> Result<Option<IncomingRevision>> {
        let RevisionRequest {
            conversation,
            existing,
            sender,
            message_id,
            revision,
            text,
            edited_at_ms,
        } = request;
        let (card, _) = existing.ok_or(Error::NotFound("contact"))?;
        if &card.identity != sender {
            return Err(Error::InvalidArgument("sender is not the pinned contact".into()));
        }
        self.plan_body_revision(super::rooms::BodyRevision {
            conversation,
            sender,
            message_id,
            revision,
            text,
            edited_at_ms,
        })
    }

    pub(super) fn plan_body_revision(
        &self,
        request: super::rooms::BodyRevision<'_>,
    ) -> Result<Option<IncomingRevision>> {
        let super::rooms::BodyRevision {
            conversation,
            sender,
            message_id,
            revision,
            text,
            edited_at_ms,
        } = request;
        if revision == 0 {
            return Ok(None);
        }
        let text = text.as_deref().map(normalize_text).transpose()?;
        let id = MessageId::from_bytes(message_id);
        let deleted = text.is_none();
        let body = match &text {
            Some(value) => MessageBody::Text { text: value.clone() },
            None => MessageBody::Deleted,
        };
        let write = RevisionWrite {
            id,
            sealed: seal_body(self, &id, conversation, &body)?,
            revision,
            edited_at_ms,
            deleted,
            author_account: AccountId::from_bytes(sender.account),
            author_device: DeviceId::from_bytes(sender.device),
        };
        match self.lookup_message(&id)? {
            Some(old) => {
                if old.conversation_id != *conversation
                    || old.author_account.as_bytes() != write.author_account.as_bytes()
                    || old.author_device.as_bytes() != write.author_device.as_bytes()
                {
                    return Err(Error::InvalidArgument("conflicting message id".into()));
                }
                if old.deleted || revision <= old.revision {
                    return Ok(None);
                }
                Ok(Some(IncomingRevision::Apply(write)))
            }
            None => Ok(Some(IncomingRevision::Hold(write))),
        }
    }

    pub(super) fn seal_record<T: Serialize>(&self, purpose: &str, id: &[u8], value: &T) -> Result<SealedBody> {
        let bytes = zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(|_| Error::Internal("record encoding"))?);
        self.cipher.seal_record(purpose, id, &bytes)
    }

    pub(super) fn read_record<T: serde::de::DeserializeOwned>(
        &self,
        purpose: &str,
        id: &[u8],
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<T> {
        let bytes = zeroize::Zeroizing::new(self.cipher.open_record(purpose, id, nonce, ciphertext)?);
        serde_json::from_slice(&bytes).map_err(|_| Error::Corrupted("record encoding"))
    }
}

pub(super) struct RevisionWrite {
    id: MessageId,
    sealed: SealedBody,
    revision: u32,
    edited_at_ms: i64,
    deleted: bool,
    author_account: AccountId,
    author_device: DeviceId,
}

pub(super) enum IncomingRevision {
    Apply(RevisionWrite),
    /// The text is not stored yet. Kept until that message arrives.
    Hold(RevisionWrite),
}

struct RevisionRequest<'a> {
    conversation: &'a ConversationId,
    existing: Option<&'a (ContactCard, bool)>,
    sender: &'a orbit_protocol::envelope::DeviceIdentity,
    message_id: [u8; 16],
    revision: u32,
    text: Option<String>,
    edited_at_ms: i64,
}

pub(super) fn seal_body(
    store: &Store,
    id: &MessageId,
    conversation: &ConversationId,
    body: &MessageBody,
) -> Result<SealedBody> {
    let bytes = zeroize::Zeroizing::new(serde_json::to_vec(body).map_err(|_| Error::Internal("body encoding"))?);
    store.cipher.seal(id, conversation, &bytes)
}

fn apply_revision(tx: &Transaction<'_>, write: &RevisionWrite) -> Result<bool> {
    let changed = tx.execute(
        "UPDATE messages SET body_nonce=?1, body_ciphertext=?2, revision=?3, edited_at_ms=?4, deleted=?5 \
         WHERE id=?6 AND author_account=?7 AND author_device=?8 AND deleted=0 AND revision < ?3",
        params![
            write.sealed.nonce.as_slice(),
            write.sealed.ciphertext,
            i64::from(write.revision),
            write.edited_at_ms,
            i64::from(write.deleted),
            write.id.as_bytes().as_slice(),
            write.author_account.as_bytes().as_slice(),
            write.author_device.as_bytes().as_slice(),
        ],
    )?;
    tx.execute(
        "DELETE FROM pending_message_ops WHERE message_id=?1",
        [write.id.as_bytes().as_slice()],
    )?;
    Ok(changed > 0)
}

fn hold_revision(tx: &Transaction<'_>, conversation: &ConversationId, write: &RevisionWrite) -> Result<()> {
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pending_message_ops WHERE message_id=?1)",
        [write.id.as_bytes().as_slice()],
        |row| row.get(0),
    )?;
    if !exists {
        let count: i64 = tx.query_row("SELECT count(*) FROM pending_message_ops", [], |row| row.get(0))?;
        if count >= 64 {
            return Ok(());
        }
    }
    tx.execute(
        "INSERT INTO pending_message_ops \
            (message_id, conversation_id, author_account, author_device, revision, deleted, edited_at_ms, body_nonce, body_ciphertext) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) \
         ON CONFLICT(message_id) DO UPDATE SET \
            conversation_id=excluded.conversation_id, \
            author_account=excluded.author_account, \
            author_device=excluded.author_device, \
            revision=excluded.revision, \
            deleted=excluded.deleted, \
            edited_at_ms=excluded.edited_at_ms, \
            body_nonce=excluded.body_nonce, \
            body_ciphertext=excluded.body_ciphertext \
         WHERE excluded.revision > pending_message_ops.revision",
        params![
            write.id.as_bytes().as_slice(),
            conversation.as_bytes().as_slice(),
            write.author_account.as_bytes().as_slice(),
            write.author_device.as_bytes().as_slice(),
            i64::from(write.revision),
            i64::from(write.deleted),
            write.edited_at_ms,
            write.sealed.nonce.as_slice(),
            write.sealed.ciphertext,
        ],
    )?;
    Ok(())
}

fn promote_pending(tx: &Transaction<'_>, id: &MessageId, account: &AccountId, device: &DeviceId) -> Result<()> {
    let row = tx
        .query_row(
            "SELECT revision, deleted, edited_at_ms, body_nonce, body_ciphertext, author_account, author_device \
             FROM pending_message_ops WHERE message_id=?1",
            [id.as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((revision, deleted, edited_at, nonce, ciphertext, author_account, author_device)) = row else {
        return Ok(());
    };
    if author_account.as_slice() == account.as_bytes() && author_device.as_slice() == device.as_bytes() {
        tx.execute(
            "UPDATE messages SET body_nonce=?1, body_ciphertext=?2, revision=?3, edited_at_ms=?4, deleted=?5 \
             WHERE id=?6 AND author_account=?7 AND author_device=?8 AND deleted=0 AND revision < ?3",
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
        "DELETE FROM pending_message_ops WHERE message_id=?1",
        [id.as_bytes().as_slice()],
    )?;
    Ok(())
}

fn check_pinned(existing: Option<&(ContactCard, bool)>, card: &ContactCard) -> Result<()> {
    if let Some((old, _)) = existing
        && (old.identity != card.identity || old.inbox_key != card.inbox_key)
    {
        return Err(Error::InvalidArgument("contact keys have changed".into()));
    }
    Ok(())
}

pub(super) fn seal(identity: &LocalIdentity, card: &ContactCard, now: i64, payload: &Payload) -> Result<Vec<u8>> {
    envelope::seal(
        identity.device_key(),
        &identity.public().to_device_identity(),
        &card.identity.device,
        &card.inbox_key,
        now,
        payload,
    )
    .map_err(|_| Error::InvalidArgument("cannot encrypt envelope for contact".into()))
}

fn insert_contact(
    tx: &Transaction<'_>,
    id: &ConversationId,
    card: &ContactCard,
    sealed: &SealedBody,
    ready: bool,
    now: i64,
) -> Result<()> {
    let count: i64 = tx.query_row("SELECT count(*) FROM contacts", [], |r| r.get(0))?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM contacts WHERE conversation_id=?1)",
        [id.as_bytes().as_slice()],
        |r| r.get(0),
    )?;
    if count >= 1000 && !exists {
        return Err(Error::Busy);
    }
    tx.execute(
        "INSERT OR IGNORE INTO conversations (id,kind,created_at_ms) VALUES (?1,?2,?3)",
        params![id.as_bytes().as_slice(), ConversationKind::Direct.as_str(), now],
    )?;
    tx.execute("INSERT INTO contacts (conversation_id,device_id,nonce,ciphertext,ready) VALUES (?1,?2,?3,?4,?5)
        ON CONFLICT(conversation_id) DO UPDATE SET nonce=excluded.nonce,ciphertext=excluded.ciphertext,ready=max(contacts.ready,excluded.ready)",
        params![id.as_bytes().as_slice(), card.identity.device.as_slice(), sealed.nonce.as_slice(), sealed.ciphertext, ready])?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn insert_message(
    tx: &Transaction<'_>,
    id: &MessageId,
    conversation: &ConversationId,
    account: &AccountId,
    device: &DeviceId,
    now: i64,
    sealed: &SealedBody,
    state: MessageState,
) -> Result<()> {
    tx.execute("INSERT INTO messages (id,conversation_id,author_account,author_device,created_at_ms,body_nonce,body_ciphertext,state)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![id.as_bytes().as_slice(), conversation.as_bytes().as_slice(), account.as_bytes().as_slice(), device.as_bytes().as_slice(), now, sealed.nonce.as_slice(), sealed.ciphertext, state.as_str()])?;
    Ok(())
}

pub(super) fn insert_outbox(
    tx: &Transaction<'_>,
    id: &MessageId,
    conversation: &ConversationId,
    message: Option<&MessageId>,
    envelope: &[u8],
    route: &SealedBody,
) -> Result<()> {
    // Preserve original ciphertext on retry: never replace an existing envelope.
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM outbox WHERE id=?1)",
        [id.as_bytes().as_slice()],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(());
    }
    let count: i64 = tx.query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))?;
    if count >= MAX_PENDING {
        return Err(Error::Busy);
    }
    tx.execute("INSERT INTO outbox (id,conversation_id,message_id,envelope,route_nonce,route_ciphertext) VALUES (?1,?2,?3,?4,?5,?6)",
        params![id.as_bytes().as_slice(), conversation.as_bytes().as_slice(), message.map(|m| m.as_bytes().as_slice()), envelope, route.nonce.as_slice(), route.ciphertext])?;
    Ok(())
}
