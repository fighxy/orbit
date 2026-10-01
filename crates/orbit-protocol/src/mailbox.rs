//! Mailbox (store-and-forward) protocol, ALPN `orbit/mailbox/1`.
//!
//! Each request travels on its own QUIC bidirectional stream: the client
//! writes one encoded [`Request`] and finishes the stream; the node answers
//! with one encoded [`Response`]. Requests on one connection share its
//! authentication state.
//!
//! * A mailbox is addressed by its owner's Ed25519 public key. The owner
//!   proves possession by signing [`auth_message`] over a fresh node nonce,
//!   bound to the node and to the client's own transport key, so a captured
//!   signature is useless on any other connection.
//! * Senders need a deposit token: a random secret the owner hands out (for
//!   example in an invitation) and registers at the node by its hash.
//!   Revoking the token stops further deposits.
//! * Envelopes are opaque to the node. Their encryption is the client's
//!   responsibility; the node only sees sizes, timing and the mailbox ID.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const ALPN: &[u8] = b"orbit/mailbox/1";

/// Largest envelope accepted by a node.
pub const MAX_ENVELOPE_BYTES: usize = 64 * 1024;
/// Largest encoded request; leaves room for the deposit header.
pub const MAX_REQUEST_BYTES: usize = MAX_ENVELOPE_BYTES + 1024;
/// Largest encoded response a client reads.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
/// Most items returned by one fetch.
pub const MAX_FETCH_ITEMS: u32 = 100;
/// Most item IDs in one acknowledgement.
pub const MAX_ACK_ITEMS: usize = 500;
/// Longest time a node holds a [`Request::Wait`].
pub const MAX_WAIT_MS: u32 = 30_000;

const AUTH_LABEL: &[u8] = b"orbit/mailbox/auth/v1\0";

macro_rules! bytes32 {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub [u8; 32]);

        impl $name {
            pub fn to_hex(&self) -> String {
                hex::encode(self.0)
            }

            pub fn from_hex(text: &str) -> Option<Self> {
                let mut bytes = [0u8; 32];
                hex::decode_to_slice(text.trim(), &mut bytes).ok()?;
                Some(Self(bytes))
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), &self.to_hex()[..16])
            }
        }
    };
}

bytes32!(
    /// Mailbox address: the owner's Ed25519 public key.
    MailboxId
);
bytes32!(
    /// Content address of a stored envelope: BLAKE3 of its bytes. Retried
    /// deposits of the same envelope are deduplicated by it.
    ItemId
);
bytes32!(
    /// BLAKE3 hash of a [`DepositToken`]; the only form the node stores.
    TokenHash
);

/// Secret that allows depositing into one mailbox. Not printed by `Debug`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositToken(pub [u8; 32]);

impl DepositToken {
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)?;
        Ok(Self(bytes))
    }

    pub fn hash(&self) -> TokenHash {
        TokenHash(*blake3::hash(&self.0).as_bytes())
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(text: &str) -> Option<Self> {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(text.trim(), &mut bytes).ok()?;
        Some(Self(bytes))
    }
}

impl std::fmt::Debug for DepositToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DepositToken([redacted])")
    }
}

impl ItemId {
    pub fn of(envelope: &[u8]) -> Self {
        Self(*blake3::hash(envelope).as_bytes())
    }
}

/// Ed25519 signature carried as bytes (serde has no 64-byte array support).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureBytes(#[serde(with = "serde_bytes")] pub Vec<u8>);

impl std::fmt::Debug for SignatureBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SignatureBytes({} bytes)", self.0.len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    /// Asks for a single-use nonce for [`Request::Authenticate`].
    Challenge,
    /// Proves ownership of `mailbox` for the rest of the connection. Creates
    /// the mailbox on first use; nodes may require `registration_code` then.
    Authenticate {
        mailbox: MailboxId,
        signature: SignatureBytes,
        registration_code: Option<String>,
    },
    /// Allows holders of the token with this hash to deposit (owner only).
    AddDepositToken { token_hash: TokenHash },
    /// Revokes a deposit token (owner only).
    RemoveDepositToken { token_hash: TokenHash },
    /// Stores an envelope for the mailbox owner. Needs no authentication.
    Deposit {
        mailbox: MailboxId,
        token: DepositToken,
        #[serde(with = "serde_bytes")]
        envelope: Vec<u8>,
    },
    /// Returns stored items with `seq > after_seq`, oldest first (owner only).
    Fetch { after_seq: u64, limit: u32 },
    /// Deletes delivered items once the client stored them durably (owner only).
    Ack { ids: Vec<ItemId> },
    /// Usage and limits of the authenticated mailbox (owner only).
    Status,
    /// Long poll (owner only): answers as soon as the mailbox holds an item
    /// with `seq > after_seq`, or after `timeout_ms` (at most
    /// [`MAX_WAIT_MS`]). Has no side effects, so nodes serve it alongside
    /// other requests of the connection; one wait per connection at a time.
    Wait { after_seq: u64, timeout_ms: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Challenge {
        nonce: [u8; 32],
    },
    Authenticated {
        created: bool,
    },
    Ok,
    Deposited {
        id: ItemId,
        /// True when this envelope was already stored; nothing was added.
        duplicate: bool,
        expires_at_ms: i64,
    },
    Items {
        items: Vec<Item>,
        more: bool,
    },
    Acked {
        removed: u32,
    },
    Status(MailboxStatus),
    /// Result of [`Request::Wait`]: `ready` is false when the wait timed out.
    Waited {
        ready: bool,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: ItemId,
    /// Node-local, strictly increasing position in this mailbox.
    pub seq: u64,
    pub received_at_ms: i64,
    pub expires_at_ms: i64,
    #[serde(with = "serde_bytes")]
    pub envelope: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxStatus {
    pub items: u64,
    pub bytes: u64,
    pub max_items: u64,
    pub max_bytes: u64,
    pub deposit_tokens: u32,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    /// Malformed or out-of-range request.
    BadRequest,
    /// The request needs an authenticated connection.
    Unauthenticated,
    /// Bad signature, unknown deposit token or missing registration code.
    Forbidden,
    /// The mailbox does not exist.
    NotFound,
    /// The mailbox or the node is full.
    QuotaExceeded,
    /// The envelope or request is too large.
    TooLarge,
    /// Too many requests on this connection.
    RateLimited,
    Internal,
}

/// Bytes signed by the mailbox owner to authenticate a connection.
pub fn auth_message(node_id: &[u8; 32], client_id: &[u8; 32], mailbox: &MailboxId, nonce: &[u8; 32]) -> Vec<u8> {
    let mut message = Vec::with_capacity(AUTH_LABEL.len() + 4 * 32);
    message.extend_from_slice(AUTH_LABEL);
    message.extend_from_slice(node_id);
    message.extend_from_slice(client_id);
    message.extend_from_slice(&mailbox.0);
    message.extend_from_slice(nonce);
    message
}

/// Signs [`auth_message`] with the mailbox key.
pub fn sign_auth(key: &SigningKey, node_id: &[u8; 32], client_id: &[u8; 32], nonce: &[u8; 32]) -> SignatureBytes {
    let mailbox = MailboxId(key.verifying_key().to_bytes());
    let signature = key.sign(&auth_message(node_id, client_id, &mailbox, nonce));
    SignatureBytes(signature.to_bytes().to_vec())
}

/// Verifies an authentication signature; strict verification rejects
/// malleable and small-order encodings.
pub fn verify_auth(
    node_id: &[u8; 32],
    client_id: &[u8; 32],
    mailbox: &MailboxId,
    nonce: &[u8; 32],
    signature: &SignatureBytes,
) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(&mailbox.0) else {
        return false;
    };
    let Ok(bytes) = <[u8; 64]>::try_from(signature.0.as_slice()) else {
        return false;
    };
    key.verify_strict(
        &auth_message(node_id, client_id, mailbox, nonce),
        &Signature::from_bytes(&bytes),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode, encode};

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn auth_signature_is_bound_to_node_client_and_nonce() {
        let key = key();
        let mailbox = MailboxId(key.verifying_key().to_bytes());
        let (node, client, nonce) = ([1u8; 32], [2u8; 32], [3u8; 32]);
        let signature = sign_auth(&key, &node, &client, &nonce);
        assert!(verify_auth(&node, &client, &mailbox, &nonce, &signature));
        assert!(!verify_auth(&[9u8; 32], &client, &mailbox, &nonce, &signature));
        assert!(!verify_auth(&node, &[9u8; 32], &mailbox, &nonce, &signature));
        assert!(!verify_auth(&node, &client, &mailbox, &[9u8; 32], &signature));
        let other = MailboxId(SigningKey::from_bytes(&[8u8; 32]).verifying_key().to_bytes());
        assert!(!verify_auth(&node, &client, &other, &nonce, &signature));
        assert!(!verify_auth(
            &node,
            &client,
            &mailbox,
            &nonce,
            &SignatureBytes(vec![0; 10])
        ));
    }

    #[test]
    fn requests_round_trip_and_reject_trailing_bytes() {
        let request = Request::Deposit {
            mailbox: MailboxId([1; 32]),
            token: DepositToken([2; 32]),
            envelope: vec![3; 1000],
        };
        let mut bytes = encode(&request).unwrap();
        // Bytes are length-prefixed rather than one varint per element.
        assert!(bytes.len() < 1100);
        assert_eq!(decode::<Request>(&bytes).unwrap(), request);
        bytes.push(0);
        assert!(decode::<Request>(&bytes).is_err());
        assert!(decode::<Request>(&[0xff, 0xff]).is_err());
    }

    #[test]
    fn largest_deposit_fits_the_request_limit() {
        let request = Request::Deposit {
            mailbox: MailboxId([1; 32]),
            token: DepositToken([2; 32]),
            envelope: vec![0; MAX_ENVELOPE_BYTES],
        };
        assert!(encode(&request).unwrap().len() <= MAX_REQUEST_BYTES);
    }

    #[test]
    fn token_hash_and_debug_do_not_reveal_the_token() {
        let token = DepositToken([5; 32]);
        assert_ne!(token.hash().0, token.0);
        assert_eq!(format!("{token:?}"), "DepositToken([redacted])");
        assert_eq!(DepositToken::from_hex(&token.to_hex()), Some(token));
    }

    #[test]
    fn item_id_is_content_address() {
        assert_eq!(ItemId::of(b"a"), ItemId::of(b"a"));
        assert_ne!(ItemId::of(b"a"), ItemId::of(b"b"));
    }
}
