//! End-to-end envelope `orbit/envelope/1` and contact invitations.
//!
//! Interim scheme (ADR 0002), used for one-to-one chats until MLS replaces it:
//!
//! * Every device has a static X25519 *inbox key*. A sender encrypts each
//!   envelope to it with HPKE (RFC 9180, base mode,
//!   DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256 / ChaCha20-Poly1305).
//! * Inside the ciphertext the sender's device key signs the payload together
//!   with the recipient device and send time, and the account key certifies
//!   the device key. The node sees neither sender nor content.
//! * No forward secrecy: whoever obtains a device's inbox key can decrypt
//!   envelopes to it that they recorded earlier.
//!
//! An [`Invite`] is a device-signed [`ContactCard`]: everything needed to
//! reach its author (identity, inbox key, node, mailbox, deposit token).

use std::collections::HashSet;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use serde::{Deserialize, Serialize};

use crate::mailbox::{DepositToken, MAX_ENVELOPE_BYTES, MailboxId, SignatureBytes};
use crate::{decode, encode};

pub const ENVELOPE_VERSION: u8 = 1;
/// HPKE suite of [`ENVELOPE_VERSION`] 1; the only one so far.
pub const SUITE_X25519_CHACHA20POLY1305: u8 = 1;
/// Text form of an invitation: this prefix and base64url of the encoding.
pub const INVITE_PREFIX: &str = "orbit://invite/";
/// Longest accepted invitation text.
pub const MAX_INVITE_TEXT: usize = 4096;
/// Longest display name carried in a card, in bytes.
pub const MAX_CARD_NAME_BYTES: usize = 256;
/// Longest node address carried in a card, in bytes.
pub const MAX_CARD_NODE_BYTES: usize = 512;
/// Most message IDs in one delivery receipt.
pub const MAX_RECEIPT_IDS: usize = 256;
/// Members of one pairwise group or channel, including the creator.
pub const MAX_ROOM_MEMBERS: usize = 8;
/// Smallest room: the creator and one other member.
pub const MIN_ROOM_MEMBERS: usize = 2;

const HPKE_INFO: &[u8] = b"orbit/envelope/v1";
const ENVELOPE_SIGNATURE_LABEL: &[u8] = b"orbit/envelope/signature/v1\0";
const INVITE_SIGNATURE_LABEL: &[u8] = b"orbit/invite/v1\0";
/// Same label as the device certificate in `orbit-core`.
const DEVICE_CERTIFICATE_LABEL: &[u8] = b"orbit/v1/device-certificate\0";
const INVITE_FORMAT: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EnvelopeError {
    #[error("malformed envelope or invitation")]
    Malformed,
    #[error("unsupported envelope or invitation version")]
    UnsupportedVersion,
    #[error("envelope cannot be decrypted with this inbox key")]
    Decrypt,
    #[error("envelope is addressed to another device")]
    WrongRecipient,
    #[error("signature or device certificate is invalid")]
    BadSignature,
    #[error("invitation has expired")]
    Expired,
    #[error("envelope exceeds the size limit")]
    TooLarge,
    #[error("encryption failed")]
    Encrypt,
}

pub type Result<T> = std::result::Result<T, EnvelopeError>;

/// Device's static HPKE key pair. Debug output never shows the secret.
pub struct InboxKey {
    secret: <X25519HkdfSha256 as Kem>::PrivateKey,
    public: [u8; 32],
}

impl InboxKey {
    /// Derives the key pair from 32 bytes of secret keying material
    /// (RFC 9180 DeriveKeyPair).
    pub fn derive(ikm: &[u8; 32]) -> Self {
        let (secret, public) = X25519HkdfSha256::derive_keypair(ikm);
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&public.to_bytes());
        Self { secret, public: bytes }
    }

    pub fn public(&self) -> [u8; 32] {
        self.public
    }
}

impl std::fmt::Debug for InboxKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InboxKey(public {})", hex::encode(self.public))
    }
}

/// Account and device of a sender, with the account's certificate over the device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub account: [u8; 32],
    pub device: [u8; 32],
    pub certificate: SignatureBytes,
}

impl DeviceIdentity {
    /// Checks that the account key certified the device key.
    pub fn verify(&self) -> Result<()> {
        let mut message = Vec::with_capacity(DEVICE_CERTIFICATE_LABEL.len() + 64);
        message.extend_from_slice(DEVICE_CERTIFICATE_LABEL);
        message.extend_from_slice(&self.account);
        message.extend_from_slice(&self.device);
        verify(&self.account, &message, &self.certificate)
    }
}

/// What a device hands out so others can write to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactCard {
    pub identity: DeviceIdentity,
    /// X25519 public key envelopes to this device are encrypted to.
    pub inbox_key: [u8; 32],
    /// Text form of the node address (see `NodeAddress`).
    pub node: String,
    pub mailbox: MailboxId,
    /// Allows depositing into `mailbox`. Secret between the two parties.
    pub token: DepositToken,
    pub display_name: String,
}

impl ContactCard {
    fn check_limits(&self) -> Result<()> {
        if self.display_name.len() > MAX_CARD_NAME_BYTES || self.node.len() > MAX_CARD_NODE_BYTES {
            return Err(EnvelopeError::TooLarge);
        }
        Ok(())
    }
}

/// Pairwise room. Postcard discriminants are append-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomKind {
    Group,
    Channel,
}

/// Application content of an envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Payload {
    /// First envelope after accepting `invite_id`: the sender's own card.
    ContactRequest {
        invite_id: [u8; 16],
        card: ContactCard,
    },
    /// Replaces the sender's card, for example with a per-contact token.
    CardUpdate {
        card: ContactCard,
    },
    Text {
        message_id: [u8; 16],
        text: String,
    },
    /// The recipient stored these messages durably.
    Delivered {
        message_ids: Vec<[u8; 16]>,
    },
    /// Replaces the author's own text. A `revision` that is not higher is ignored.
    /// Appended so existing postcard indexes stay valid.
    EditText {
        message_id: [u8; 16],
        revision: u32,
        text: String,
    },
    /// Removes the author's own text. Later edits of that message are ignored.
    DeleteText {
        message_id: [u8; 16],
        revision: u32,
    },
    /// Membership list sealed separately for each member. Not an MLS welcome:
    /// there is no shared epoch, and a later list does not erase old copies.
    RoomWelcome {
        room_id: [u8; 16],
        kind: RoomKind,
        title: String,
        members: Vec<ContactCard>,
    },
    RoomText {
        room_id: [u8; 16],
        message_id: [u8; 16],
        text: String,
    },
    RoomEditText {
        room_id: [u8; 16],
        message_id: [u8; 16],
        revision: u32,
        text: String,
    },
    RoomDeleteText {
        room_id: [u8; 16],
        message_id: [u8; 16],
        revision: u32,
    },
    /// Profile picture sent after contact exchange. Empty clears it.
    /// Kept out of the invite text, which is limited to 4 KiB.
    Avatar {
        image: Vec<u8>,
    },
    /// Header for a voice note split across following [`Payload::MediaChunk`]s.
    /// A note is a PCM WAV, not a live room and not a video circle.
    MediaStart {
        message_id: [u8; 16],
        duration_ms: u32,
        byte_len: u32,
        sha256: [u8; 32],
        chunk_count: u16,
        waveform: Vec<u8>,
    },
    /// One slice of the WAV named by [`Payload::MediaStart`].
    MediaChunk {
        message_id: [u8; 16],
        index: u16,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
}

/// Largest voice note, about 60 seconds of 16 kHz mono 16-bit PCM.
pub const MAX_VOICE_BYTES: usize = 2 * 1024 * 1024;

/// Longest voice note.
pub const MAX_VOICE_MS: u32 = 60_000;

/// Plaintext of one voice slice. The sealed envelope stays under 64 KiB.
pub const MAX_VOICE_CHUNK_BYTES: usize = 32 * 1024;

/// Waveform bars carried beside a voice note.
pub const MAX_WAVEFORM_BARS: usize = 48;

/// Upper bound on slices for one note.
pub const MAX_VOICE_CHUNKS: usize = MAX_VOICE_BYTES.div_ceil(MAX_VOICE_CHUNK_BYTES);

/// Largest profile picture carried in one envelope.
pub const MAX_AVATAR_BYTES: usize = 32 * 1024;

/// JPEG or PNG small enough for one direct envelope.
pub fn valid_avatar(image: &[u8]) -> bool {
    if image.is_empty() || image.len() > MAX_AVATAR_BYTES {
        return false;
    }
    let jpeg = image.len() >= 3 && image[0] == 0xFF && image[1] == 0xD8 && image[2] == 0xFF;
    let png = image.starts_with(b"\x89PNG\r\n\x1a\n");
    jpeg || png
}

/// Decrypted and verified envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub sender: DeviceIdentity,
    /// Sender's wall clock; display and ordering hints only.
    pub sent_at_ms: i64,
    pub payload: Payload,
}

#[derive(Serialize, Deserialize)]
struct Outer {
    version: u8,
    suite: u8,
    #[serde(with = "serde_bytes")]
    enc: Vec<u8>,
    #[serde(with = "serde_bytes")]
    ciphertext: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Inner {
    sender: DeviceIdentity,
    recipient_device: [u8; 32],
    sent_at_ms: i64,
    #[serde(with = "serde_bytes")]
    payload: Vec<u8>,
    signature: SignatureBytes,
}

fn envelope_signed_bytes(
    sender: &DeviceIdentity,
    recipient_device: &[u8; 32],
    sent_at_ms: i64,
    payload: &[u8],
) -> Vec<u8> {
    let mut message = Vec::with_capacity(ENVELOPE_SIGNATURE_LABEL.len() + 104 + payload.len());
    message.extend_from_slice(ENVELOPE_SIGNATURE_LABEL);
    message.extend_from_slice(&sender.account);
    message.extend_from_slice(&sender.device);
    message.extend_from_slice(recipient_device);
    message.extend_from_slice(&sent_at_ms.to_be_bytes());
    message.extend_from_slice(payload);
    message
}

/// Signs and encrypts `payload` for one recipient device.
pub fn seal(
    device_key: &SigningKey,
    sender: &DeviceIdentity,
    recipient_device: &[u8; 32],
    recipient_inbox: &[u8; 32],
    sent_at_ms: i64,
    payload: &Payload,
) -> Result<Vec<u8>> {
    if device_key.verifying_key().to_bytes() != sender.device {
        return Err(EnvelopeError::BadSignature);
    }
    let payload = encode(payload).map_err(|_| EnvelopeError::Malformed)?;
    let signature = device_key.sign(&envelope_signed_bytes(sender, recipient_device, sent_at_ms, &payload));
    let inner = encode(&Inner {
        sender: sender.clone(),
        recipient_device: *recipient_device,
        sent_at_ms,
        payload,
        signature: SignatureBytes(signature.to_bytes().to_vec()),
    })
    .map_err(|_| EnvelopeError::Malformed)?;

    let recipient =
        <X25519HkdfSha256 as Kem>::PublicKey::from_bytes(recipient_inbox).map_err(|_| EnvelopeError::Encrypt)?;
    let aad = [ENVELOPE_VERSION, SUITE_X25519_CHACHA20POLY1305];
    let (enc, ciphertext) = hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
        &OpModeS::Base,
        &recipient,
        HPKE_INFO,
        &inner,
        &aad,
    )
    .map_err(|_| EnvelopeError::Encrypt)?;
    let envelope = encode(&Outer {
        version: ENVELOPE_VERSION,
        suite: SUITE_X25519_CHACHA20POLY1305,
        enc: enc.to_bytes().to_vec(),
        ciphertext,
    })
    .map_err(|_| EnvelopeError::Malformed)?;
    if envelope.len() > MAX_ENVELOPE_BYTES {
        return Err(EnvelopeError::TooLarge);
    }
    Ok(envelope)
}

/// Decrypts an envelope addressed to `own_device` and verifies its sender.
pub fn open(inbox: &InboxKey, own_device: &[u8; 32], envelope: &[u8]) -> Result<Opened> {
    if envelope.len() > MAX_ENVELOPE_BYTES {
        return Err(EnvelopeError::TooLarge);
    }
    let outer: Outer = decode(envelope).map_err(|_| EnvelopeError::Malformed)?;
    if outer.version != ENVELOPE_VERSION || outer.suite != SUITE_X25519_CHACHA20POLY1305 {
        return Err(EnvelopeError::UnsupportedVersion);
    }
    let enc = <X25519HkdfSha256 as Kem>::EncappedKey::from_bytes(&outer.enc).map_err(|_| EnvelopeError::Malformed)?;
    let aad = [outer.version, outer.suite];
    let inner = zeroize::Zeroizing::new(
        hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeR::Base,
            &inbox.secret,
            &enc,
            HPKE_INFO,
            &outer.ciphertext,
            &aad,
        )
        .map_err(|_| EnvelopeError::Decrypt)?,
    );
    let inner: Inner = decode(&inner).map_err(|_| EnvelopeError::Malformed)?;
    if inner.recipient_device != *own_device {
        return Err(EnvelopeError::WrongRecipient);
    }
    inner.sender.verify()?;
    verify(
        &inner.sender.device,
        &envelope_signed_bytes(&inner.sender, &inner.recipient_device, inner.sent_at_ms, &inner.payload),
        &inner.signature,
    )?;
    let payload: Payload = decode(&inner.payload).map_err(|_| EnvelopeError::Malformed)?;
    match &payload {
        Payload::ContactRequest { card, .. } | Payload::CardUpdate { card } => {
            // A card in an envelope speaks only for its signer.
            if card.identity != inner.sender {
                return Err(EnvelopeError::BadSignature);
            }
            card.check_limits()?;
        }
        Payload::Delivered { message_ids } if message_ids.len() > MAX_RECEIPT_IDS => {
            return Err(EnvelopeError::TooLarge);
        }
        Payload::RoomWelcome { title, members, .. } => check_room_welcome(&inner.sender, title, members)?,
        Payload::Avatar { image } if image.len() > MAX_AVATAR_BYTES => return Err(EnvelopeError::TooLarge),
        Payload::Avatar { image } if !image.is_empty() && !valid_avatar(image) => {
            return Err(EnvelopeError::Malformed);
        }
        Payload::MediaStart {
            byte_len,
            duration_ms,
            chunk_count,
            waveform,
            ..
        } => {
            if *byte_len == 0 || usize::try_from(*byte_len).unwrap_or(usize::MAX) > MAX_VOICE_BYTES {
                return Err(EnvelopeError::TooLarge);
            }
            if *duration_ms == 0 || *duration_ms > MAX_VOICE_MS || waveform.len() > MAX_WAVEFORM_BARS {
                return Err(EnvelopeError::Malformed);
            }
            let count = usize::from(*chunk_count);
            let needed = usize::try_from(*byte_len)
                .unwrap_or(usize::MAX)
                .div_ceil(MAX_VOICE_CHUNK_BYTES);
            if count == 0 || count > MAX_VOICE_CHUNKS || count < needed {
                return Err(EnvelopeError::Malformed);
            }
        }
        Payload::MediaChunk { bytes, .. } if bytes.is_empty() => return Err(EnvelopeError::Malformed),
        Payload::MediaChunk { bytes, .. } if bytes.len() > MAX_VOICE_CHUNK_BYTES => {
            return Err(EnvelopeError::TooLarge);
        }
        _ => {}
    }
    Ok(Opened {
        sender: inner.sender,
        sent_at_ms: inner.sent_at_ms,
        payload,
    })
}

/// Device-signed invitation to become a contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub invite_id: [u8; 16],
    pub expires_at_ms: i64,
    pub card: ContactCard,
    signature: SignatureBytes,
}

impl Invite {
    pub fn create(device_key: &SigningKey, invite_id: [u8; 16], expires_at_ms: i64, card: ContactCard) -> Result<Self> {
        if device_key.verifying_key().to_bytes() != card.identity.device {
            return Err(EnvelopeError::BadSignature);
        }
        card.check_limits()?;
        let signature = device_key.sign(&Self::signed_bytes(&invite_id, expires_at_ms, &card)?);
        Ok(Self {
            invite_id,
            expires_at_ms,
            card,
            signature: SignatureBytes(signature.to_bytes().to_vec()),
        })
    }

    fn signed_bytes(invite_id: &[u8; 16], expires_at_ms: i64, card: &ContactCard) -> Result<Vec<u8>> {
        let body = encode(&(invite_id, expires_at_ms, card)).map_err(|_| EnvelopeError::Malformed)?;
        let mut message = Vec::with_capacity(INVITE_SIGNATURE_LABEL.len() + body.len());
        message.extend_from_slice(INVITE_SIGNATURE_LABEL);
        message.extend_from_slice(&body);
        Ok(message)
    }

    pub fn to_text(&self) -> String {
        let mut bytes = vec![INVITE_FORMAT];
        // Encoding a struct of plain fields cannot fail.
        bytes.extend(encode(self).unwrap_or_default());
        format!("{INVITE_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Parses and verifies an invitation; surrounding whitespace is ignored.
    pub fn parse(text: &str, now_ms: i64) -> Result<Self> {
        let text = text.trim();
        if text.len() > MAX_INVITE_TEXT {
            return Err(EnvelopeError::TooLarge);
        }
        let encoded = text.strip_prefix(INVITE_PREFIX).ok_or(EnvelopeError::Malformed)?;
        let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| EnvelopeError::Malformed)?;
        let (&format, body) = bytes.split_first().ok_or(EnvelopeError::Malformed)?;
        if format != INVITE_FORMAT {
            return Err(EnvelopeError::UnsupportedVersion);
        }
        let invite: Invite = decode(body).map_err(|_| EnvelopeError::Malformed)?;
        invite.card.check_limits()?;
        invite.card.identity.verify()?;
        verify(
            &invite.card.identity.device,
            &Self::signed_bytes(&invite.invite_id, invite.expires_at_ms, &invite.card)?,
            &invite.signature,
        )?;
        if invite.expires_at_ms <= now_ms {
            return Err(EnvelopeError::Expired);
        }
        Ok(invite)
    }
}

fn check_room_welcome(sender: &DeviceIdentity, title: &str, members: &[ContactCard]) -> Result<()> {
    if title.is_empty() || title.len() > MAX_CARD_NAME_BYTES {
        return Err(EnvelopeError::Malformed);
    }
    if !(MIN_ROOM_MEMBERS..=MAX_ROOM_MEMBERS).contains(&members.len()) {
        return Err(EnvelopeError::TooLarge);
    }
    let mut devices = HashSet::with_capacity(members.len());
    let mut includes_sender = false;
    for card in members {
        card.check_limits()?;
        card.identity.verify()?;
        if !devices.insert(card.identity.device) {
            return Err(EnvelopeError::Malformed);
        }
        if card.identity == *sender {
            includes_sender = true;
        }
    }
    if !includes_sender {
        return Err(EnvelopeError::BadSignature);
    }
    Ok(())
}

fn verify(public_key: &[u8; 32], message: &[u8], signature: &SignatureBytes) -> Result<()> {
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| EnvelopeError::BadSignature)?;
    let bytes = <[u8; 64]>::try_from(signature.0.as_slice()).map_err(|_| EnvelopeError::BadSignature)?;
    key.verify_strict(message, &Signature::from_bytes(&bytes))
        .map_err(|_| EnvelopeError::BadSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Party {
        account: SigningKey,
        device: SigningKey,
        inbox: InboxKey,
    }

    impl Party {
        fn new(seed: u8) -> Self {
            Self {
                account: SigningKey::from_bytes(&[seed; 32]),
                device: SigningKey::from_bytes(&[seed + 1; 32]),
                inbox: InboxKey::derive(&[seed + 2; 32]),
            }
        }

        fn identity(&self) -> DeviceIdentity {
            let account = self.account.verifying_key().to_bytes();
            let device = self.device.verifying_key().to_bytes();
            let mut message = DEVICE_CERTIFICATE_LABEL.to_vec();
            message.extend_from_slice(&account);
            message.extend_from_slice(&device);
            DeviceIdentity {
                account,
                device,
                certificate: SignatureBytes(self.account.sign(&message).to_bytes().to_vec()),
            }
        }

        fn device_id(&self) -> [u8; 32] {
            self.device.verifying_key().to_bytes()
        }

        fn card(&self) -> ContactCard {
            ContactCard {
                identity: self.identity(),
                inbox_key: self.inbox.public(),
                node: format!("{}@127.0.0.1:7443", "ab".repeat(32)),
                mailbox: MailboxId([9; 32]),
                token: DepositToken([7; 32]),
                display_name: "Анна".into(),
            }
        }
    }

    fn text(id: u8) -> Payload {
        Payload::Text {
            message_id: [id; 16],
            text: "привет".into(),
        }
    }

    fn seal_from(sender: &Party, recipient: &Party, payload: &Payload) -> Vec<u8> {
        seal(
            &sender.device,
            &sender.identity(),
            &recipient.device_id(),
            &recipient.inbox.public(),
            1_000,
            payload,
        )
        .unwrap()
    }

    #[test]
    fn round_trip_reveals_payload_only_to_recipient() {
        let (alice, bob, eve) = (Party::new(10), Party::new(20), Party::new(30));
        let envelope = seal_from(&alice, &bob, &text(1));
        assert!(!envelope.windows(12).any(|w| w == "привет".as_bytes()));

        let opened = open(&bob.inbox, &bob.device_id(), &envelope).unwrap();
        assert_eq!(opened.sender, alice.identity());
        assert_eq!(opened.sent_at_ms, 1_000);
        assert_eq!(opened.payload, text(1));

        assert_eq!(
            open(&eve.inbox, &eve.device_id(), &envelope),
            Err(EnvelopeError::Decrypt)
        );
        // Same inbox key claimed by another device ID: refused.
        assert_eq!(
            open(&bob.inbox, &eve.device_id(), &envelope),
            Err(EnvelopeError::WrongRecipient)
        );
    }

    #[test]
    fn sealing_is_randomized() {
        let (alice, bob) = (Party::new(10), Party::new(20));
        assert_ne!(seal_from(&alice, &bob, &text(1)), seal_from(&alice, &bob, &text(1)));
    }

    #[test]
    fn tampering_is_detected() {
        let (alice, bob) = (Party::new(10), Party::new(20));
        let envelope = seal_from(&alice, &bob, &text(1));
        for index in [0, 5, envelope.len() / 2, envelope.len() - 1] {
            let mut bad = envelope.clone();
            bad[index] ^= 1;
            assert!(open(&bob.inbox, &bob.device_id(), &bad).is_err(), "byte {index}");
        }
        assert!(open(&bob.inbox, &bob.device_id(), &envelope[..envelope.len() - 1]).is_err());
    }

    #[test]
    fn forged_sender_is_rejected() {
        let (alice, bob, eve) = (Party::new(10), Party::new(20), Party::new(30));
        // Eve signs with her device key but claims Alice's identity.
        assert!(
            seal(
                &eve.device,
                &alice.identity(),
                &bob.device_id(),
                &bob.inbox.public(),
                1,
                &text(1)
            )
            .is_err()
        );
        // A device not certified by the claimed account.
        let mut uncertified = eve.identity();
        uncertified.account = alice.identity().account;
        let envelope = seal(
            &eve.device,
            &uncertified,
            &bob.device_id(),
            &bob.inbox.public(),
            1,
            &text(1),
        )
        .unwrap();
        assert_eq!(
            open(&bob.inbox, &bob.device_id(), &envelope),
            Err(EnvelopeError::BadSignature)
        );
    }

    #[test]
    fn card_must_belong_to_the_sender() {
        let (alice, bob, eve) = (Party::new(10), Party::new(20), Party::new(30));
        let payload = Payload::CardUpdate { card: alice.card() };
        let envelope = seal_from(&eve, &bob, &payload);
        assert_eq!(
            open(&bob.inbox, &bob.device_id(), &envelope),
            Err(EnvelopeError::BadSignature)
        );
        let own = seal_from(&alice, &bob, &payload);
        assert_eq!(open(&bob.inbox, &bob.device_id(), &own).unwrap().payload, payload);
    }

    #[test]
    fn room_welcome_names_the_sender_and_round_trips() {
        let (alice, bob) = (Party::new(10), Party::new(20));
        let welcome = Payload::RoomWelcome {
            room_id: [3; 16],
            kind: RoomKind::Channel,
            title: "новости".into(),
            members: vec![alice.card(), bob.card()],
        };
        let opened = open(&bob.inbox, &bob.device_id(), &seal_from(&alice, &bob, &welcome)).unwrap();
        assert_eq!(opened.payload, welcome);

        let eve = Party::new(30);
        let without_sender = Payload::RoomWelcome {
            room_id: [3; 16],
            kind: RoomKind::Group,
            title: "кухня".into(),
            members: vec![bob.card(), eve.card()],
        };
        assert_eq!(
            open(&bob.inbox, &bob.device_id(), &seal_from(&alice, &bob, &without_sender)),
            Err(EnvelopeError::BadSignature)
        );
    }

    #[test]
    fn oversized_payload_is_refused() {
        let (alice, bob) = (Party::new(10), Party::new(20));
        let payload = Payload::Text {
            message_id: [1; 16],
            text: "x".repeat(MAX_ENVELOPE_BYTES),
        };
        assert_eq!(
            seal(
                &alice.device,
                &alice.identity(),
                &bob.device_id(),
                &bob.inbox.public(),
                1,
                &payload
            ),
            Err(EnvelopeError::TooLarge)
        );
    }

    #[test]
    fn invite_round_trips_and_verifies() {
        let alice = Party::new(10);
        let invite = Invite::create(&alice.device, [4; 16], 5_000, alice.card()).unwrap();
        let text = invite.to_text();
        assert!(text.starts_with(INVITE_PREFIX));
        assert!(text.len() < 1000, "invite text is {} bytes", text.len());
        let parsed = Invite::parse(&format!("  {text}\n"), 4_999).unwrap();
        assert_eq!(parsed, invite);
        assert_eq!(Invite::parse(&text, 5_000), Err(EnvelopeError::Expired));
    }

    #[test]
    fn modified_invite_is_rejected() {
        let alice = Party::new(10);
        let mut invite = Invite::create(&alice.device, [4; 16], 5_000, alice.card()).unwrap();
        invite.card.token = DepositToken([8; 32]);
        assert_eq!(Invite::parse(&invite.to_text(), 0), Err(EnvelopeError::BadSignature));
        assert_eq!(
            Invite::parse("orbit://invite/AAAA", 0),
            Err(EnvelopeError::UnsupportedVersion)
        );
        assert_eq!(Invite::parse("hello", 0), Err(EnvelopeError::Malformed));
        assert_eq!(Invite::parse("orbit://invite/!!", 0), Err(EnvelopeError::Malformed));
    }

    #[test]
    fn inbox_key_derivation_is_deterministic() {
        assert_eq!(InboxKey::derive(&[1; 32]).public(), InboxKey::derive(&[1; 32]).public());
        assert_ne!(InboxKey::derive(&[1; 32]).public(), InboxKey::derive(&[2; 32]).public());
        assert!(!format!("{:?}", InboxKey::derive(&[1; 32])).contains("secret"));
    }
}
