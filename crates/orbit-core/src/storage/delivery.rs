//! Single-device contact exchange and transactional mailbox delivery.
//! Secrets in contact cards/config/routes are sealed under the local device key.

use orbit_protocol::NodeAddress;
use orbit_protocol::envelope::{self, ContactCard, Invite, Payload};
use orbit_protocol::mailbox::{DepositToken, Item, ItemId, MailboxId};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::cipher::SealedBody;
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
                })
            })
            .transpose()
    }

    fn contact_card(&self, id: &ConversationId) -> Result<Option<(ContactCard, bool)>> {
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
        })
    }

    /// Sends text only after the mutually authenticated contact exchange. Handshake
    /// and receipt envelopes are not held back by a pending contact.
    pub(crate) fn outbox(&self, limit: u32) -> Result<Vec<OutboxItem>> {
        let mut statement = self.conn.prepare(
            "SELECT o.id, o.envelope, o.route_nonce, o.route_ciphertext FROM outbox o
            JOIN contacts c ON c.conversation_id=o.conversation_id
            WHERE o.message_id IS NULL OR c.ready=1 ORDER BY o.rowid LIMIT ?1",
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
        if let Some(ref message_id) = message_id {
            tx.execute(
                "UPDATE messages SET state='mailbox' WHERE id=?1 AND state='queued'",
                [message_id],
            )?;
        }
        tx.execute("DELETE FROM outbox WHERE id=?1", [id.as_bytes().as_slice()])?;
        tx.commit()?;
        message_id
            .map(|id| self.message(&MessageId::from_slice(&id).map_err(|_| Error::Corrupted("outbox message id"))?))
            .transpose()
    }

    fn message(&self, id: &MessageId) -> Result<Message> {
        let row = self.conn.query_row(
            "SELECT seq,id,conversation_id,author_account,author_device,created_at_ms,
            body_nonce,body_ciphertext,state FROM messages WHERE id=?1",
            [id.as_bytes().as_slice()],
            StoredMessage::from_row,
        )?;
        self.decode(row)
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
                if let Some(old) = self
                    .conn
                    .query_row(
                        "SELECT seq,id,conversation_id,author_account,author_device,created_at_ms,
                    body_nonce,body_ciphertext,state FROM messages WHERE id=?1",
                        [id.as_bytes().as_slice()],
                        StoredMessage::from_row,
                    )
                    .optional()?
                {
                    let old = self.decode(old)?;
                    if old.conversation_id != conversation
                        || old.author_device.as_bytes() != &opened.sender.device
                        || old.body != (MessageBody::Text { text: text.clone() })
                    {
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
                let (card, _) = existing.as_ref().ok_or(Error::NotFound("contact"))?;
                if card.identity != opened.sender {
                    return Err(Error::InvalidArgument("receipt is not from the pinned contact".into()));
                }
                delivered = message_ids.into_iter().map(MessageId::from_bytes).collect();
            }
        }
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
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let (Some(card), Some(sealed)) = (&card_change, &sealed_card) {
            insert_contact(&tx, &conversation, card, sealed, true, now)?;
        }
        let received_id = if let Some((id, sealed)) = received {
            insert_message(
                &tx,
                &id,
                &conversation,
                &AccountId::from_bytes(opened.sender.account),
                &DeviceId::from_bytes(opened.sender.device),
                opened.sent_at_ms,
                &sealed,
                MessageState::Received,
            )?;
            Some(id)
        } else {
            None
        };
        let mut changed_ids = Vec::new();
        for id in delivered {
            let changed = tx.execute("UPDATE messages SET state='delivered' WHERE id=?1 AND conversation_id=?2 AND author_device=?3 AND state IN ('queued','mailbox')",
                params![id.as_bytes().as_slice(), conversation.as_bytes().as_slice(), self.device_id.as_bytes().as_slice()])?;
            if changed > 0 {
                changed_ids.push(id);
            }
        }
        if let Some((envelope, route)) = reply {
            insert_outbox(&tx, &job_id, &conversation, None, &envelope, &route)?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO processed_inbox (id,accepted) VALUES (?1,1)",
            [item.id.0.as_slice()],
        )?;
        tx.commit()?;
        if let Some(id) = received_id {
            changed_ids.push(id);
        }
        Ok(InboxChanges {
            messages: changed_ids.iter().map(|id| self.message(id)).collect::<Result<_>>()?,
            contacts_changed: card_change.is_some(),
        })
    }

    fn seal_record<T: Serialize>(&self, purpose: &str, id: &[u8], value: &T) -> Result<SealedBody> {
        let bytes = zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(|_| Error::Internal("record encoding"))?);
        self.cipher.seal_record(purpose, id, &bytes)
    }

    fn read_record<T: serde::de::DeserializeOwned>(
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

fn check_pinned(existing: Option<&(ContactCard, bool)>, card: &ContactCard) -> Result<()> {
    if let Some((old, _)) = existing
        && (old.identity != card.identity || old.inbox_key != card.inbox_key)
    {
        return Err(Error::InvalidArgument("contact keys have changed".into()));
    }
    Ok(())
}

fn seal(identity: &LocalIdentity, card: &ContactCard, now: i64, payload: &Payload) -> Result<Vec<u8>> {
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
fn insert_message(
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

fn insert_outbox(
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
