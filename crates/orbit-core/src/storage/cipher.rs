//! Encryption of message bodies at rest.
//!
//! Bodies are sealed with XChaCha20-Poly1305 under the device storage key and
//! bound to their message and conversation IDs. Metadata needed for ordering
//! (IDs, sequence, timestamps) stays in clear inside the account database.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use subtle::ConstantTimeEq;

use crate::domain::{ConversationId, MessageId};
use crate::error::{Error, Result};

pub(crate) const NONCE_LEN: usize = 24;
const BODY_AAD_LABEL: &[u8] = b"orbit/v1/local-message-body\0";
const KEY_CHECK_LABEL: &[u8] = b"orbit/v1/local-storage-key-check";

pub(crate) struct LocalCipher {
    aead: XChaCha20Poly1305,
    key_check: [u8; 32],
}

pub(crate) struct SealedBody {
    pub nonce: [u8; NONCE_LEN],
    pub ciphertext: Vec<u8>,
}

impl LocalCipher {
    /// Protect non-message records with a separate, purpose-bound AAD domain.
    pub fn seal_record(&self, purpose: &str, id: &[u8], plaintext: &[u8]) -> Result<SealedBody> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|_| Error::Random)?;
        let aad = record_aad(purpose, id);
        let ciphertext = self
            .aead
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| Error::Internal("record encryption failed"))?;
        Ok(SealedBody { nonce, ciphertext })
    }

    pub fn open_record(&self, purpose: &str, id: &[u8], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| Error::Corrupted("record nonce"))?;
        let aad = record_aad(purpose, id);
        self.aead
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| Error::Corrupted("record authentication"))
    }
    pub fn new(storage_key: &[u8; 32]) -> Self {
        let key = Key::from(*storage_key);
        Self {
            aead: XChaCha20Poly1305::new(&key),
            key_check: *blake3::keyed_hash(storage_key, KEY_CHECK_LABEL).as_bytes(),
        }
    }

    /// Value stored on first open to detect a wrong key on later opens.
    pub fn key_check(&self) -> &[u8; 32] {
        &self.key_check
    }

    pub fn verify_key_check(&self, stored: &[u8]) -> Result<()> {
        if stored.len() == self.key_check.len() && bool::from(stored.ct_eq(&self.key_check)) {
            Ok(())
        } else {
            Err(Error::StorageKeyMismatch)
        }
    }

    pub fn seal(&self, message: &MessageId, conversation: &ConversationId, plaintext: &[u8]) -> Result<SealedBody> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|_| Error::Random)?;
        let aad = aad(message, conversation);
        let ciphertext = self
            .aead
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| Error::Internal("message body encryption failed"))?;
        Ok(SealedBody { nonce, ciphertext })
    }

    pub fn open(
        &self,
        message: &MessageId,
        conversation: &ConversationId,
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>> {
        let nonce: [u8; NONCE_LEN] = nonce
            .try_into()
            .map_err(|_| Error::Corrupted("message body nonce has a wrong length"))?;
        let aad = aad(message, conversation);
        self.aead
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| Error::Corrupted("message body failed authentication"))
    }
}

fn record_aad(purpose: &str, id: &[u8]) -> Vec<u8> {
    let mut aad = b"orbit/v1/local-record\0".to_vec();
    aad.extend_from_slice(purpose.as_bytes());
    aad.push(0);
    aad.extend_from_slice(id);
    aad
}

fn aad(message: &MessageId, conversation: &ConversationId) -> Vec<u8> {
    let mut aad = Vec::with_capacity(BODY_AAD_LABEL.len() + MessageId::LEN + ConversationId::LEN);
    aad.extend_from_slice(BODY_AAD_LABEL);
    aad.extend_from_slice(message.as_bytes());
    aad.extend_from_slice(conversation.as_bytes());
    aad
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_round_trip_and_binding() {
        let cipher = LocalCipher::new(&[3; 32]);
        let message = MessageId::from_bytes([1; 16]);
        let conversation = ConversationId::from_bytes([2; 16]);
        let sealed = cipher.seal(&message, &conversation, b"secret note").unwrap();
        assert_ne!(sealed.ciphertext, b"secret note");

        let opened = cipher
            .open(&message, &conversation, &sealed.nonce, &sealed.ciphertext)
            .unwrap();
        assert_eq!(opened, b"secret note");

        // Moving the ciphertext to another message or conversation is detected.
        let other_message = MessageId::from_bytes([9; 16]);
        assert!(
            cipher
                .open(&other_message, &conversation, &sealed.nonce, &sealed.ciphertext)
                .is_err()
        );
        let other_conversation = ConversationId::from_bytes([9; 16]);
        assert!(
            cipher
                .open(&message, &other_conversation, &sealed.nonce, &sealed.ciphertext)
                .is_err()
        );

        let wrong_key = LocalCipher::new(&[4; 32]);
        assert!(
            wrong_key
                .open(&message, &conversation, &sealed.nonce, &sealed.ciphertext)
                .is_err()
        );
    }

    #[test]
    fn key_check_detects_wrong_key() {
        let cipher = LocalCipher::new(&[3; 32]);
        cipher.verify_key_check(cipher.key_check()).unwrap();
        let other = LocalCipher::new(&[4; 32]);
        assert!(matches!(
            other.verify_key_check(cipher.key_check()),
            Err(Error::StorageKeyMismatch)
        ));
        assert!(cipher.verify_key_check(&[0; 3]).is_err());
    }
}
