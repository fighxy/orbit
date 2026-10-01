//! Account and device identity.
//!
//! The only secret that leaves the core is [`IdentitySecret`], an opaque,
//! versioned blob stored by the platform secure store (Keychain, Android
//! Keystore, desktop keyring). Everything derived from it stays in Rust.
//!
//! * The account seed defines the account signing key. Its public key is the
//!   [`AccountId`]. Backing up and restoring this seed is a later, separately
//!   specified flow.
//! * The device seed is random per device and is never derived from the
//!   account seed, so restoring an account cannot resurrect a revoked device.
//! * The account key certifies the device key ([`DeviceCertificate`]).
//! * The local storage key is derived from the device seed.
//! * So are the device's inbox key (HPKE, for end-to-end envelopes) and its
//!   mailbox key. The mailbox key is separate from the device key so the
//!   node cannot link a mailbox to a published identity by key equality.

pub mod passcode;

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use orbit_protocol::envelope::{DeviceIdentity, InboxKey};
use orbit_protocol::mailbox::SignatureBytes;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::domain::{AccountId, DeviceId};
use crate::error::{Error, Result};

const SECRET_VERSION: u8 = 1;
const SEED_LEN: usize = 32;
/// Length of the serialized identity secret, version 1.
pub const IDENTITY_SECRET_LEN: usize = 1 + 2 * SEED_LEN;

// BLAKE3 derive_key contexts: hardcoded, unique, versioned.
const ACCOUNT_SIGNING_CONTEXT: &str = "orbit 2026-10-01 account signing key v1";
const DEVICE_SIGNING_CONTEXT: &str = "orbit 2026-10-01 device signing key v1";
const LOCAL_STORAGE_CONTEXT: &str = "orbit 2026-10-01 local storage key v1";
const DEVICE_INBOX_CONTEXT: &str = "orbit 2026-10-01 device inbox key v1";
const DEVICE_MAILBOX_CONTEXT: &str = "orbit 2026-10-01 device mailbox key v1";

const DEVICE_CERTIFICATE_LABEL: &[u8] = b"orbit/v1/device-certificate\0";

/// Opaque secret material of one account on one device.
pub struct IdentitySecret {
    account_seed: Zeroizing<[u8; SEED_LEN]>,
    device_seed: Zeroizing<[u8; SEED_LEN]>,
}

impl IdentitySecret {
    /// Creates a new account with a new device using the OS random generator.
    pub fn generate() -> Result<Self> {
        let mut account_seed = Zeroizing::new([0u8; SEED_LEN]);
        let mut device_seed = Zeroizing::new([0u8; SEED_LEN]);
        getrandom::fill(account_seed.as_mut()).map_err(|_| Error::Random)?;
        getrandom::fill(device_seed.as_mut()).map_err(|_| Error::Random)?;
        Ok(Self {
            account_seed,
            device_seed,
        })
    }

    /// Parses a secret produced by [`IdentitySecret::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != IDENTITY_SECRET_LEN || bytes[0] != SECRET_VERSION {
            return Err(Error::InvalidIdentity);
        }
        let mut account_seed = Zeroizing::new([0u8; SEED_LEN]);
        let mut device_seed = Zeroizing::new([0u8; SEED_LEN]);
        account_seed.copy_from_slice(&bytes[1..1 + SEED_LEN]);
        device_seed.copy_from_slice(&bytes[1 + SEED_LEN..]);
        Ok(Self {
            account_seed,
            device_seed,
        })
    }

    pub fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(IDENTITY_SECRET_LEN));
        out.push(SECRET_VERSION);
        out.extend_from_slice(self.account_seed.as_ref());
        out.extend_from_slice(self.device_seed.as_ref());
        out
    }
}

impl fmt::Debug for IdentitySecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IdentitySecret([redacted])")
    }
}

/// Signature of the account key over a device key.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DeviceCertificate([u8; 64]);

impl DeviceCertificate {
    fn signed_bytes(account: &AccountId, device: &DeviceId) -> Vec<u8> {
        let mut message = Vec::with_capacity(DEVICE_CERTIFICATE_LABEL.len() + 64);
        message.extend_from_slice(DEVICE_CERTIFICATE_LABEL);
        message.extend_from_slice(account.as_bytes());
        message.extend_from_slice(device.as_bytes());
        message
    }

    /// Verifies that `account` certified `device`.
    pub fn verify(&self, account: &AccountId, device: &DeviceId) -> Result<()> {
        let key = VerifyingKey::from_bytes(account.as_bytes()).map_err(|_| Error::InvalidIdentity)?;
        let signature = Signature::from_bytes(&self.0);
        key.verify_strict(&Self::signed_bytes(account, device), &signature)
            .map_err(|_| Error::InvalidIdentity)
    }

    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl fmt::Debug for DeviceCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceCertificate({})", hex::encode(self.0))
    }
}

impl Serialize for DeviceCertificate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for DeviceCertificate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let mut bytes = [0u8; 64];
        hex::decode_to_slice(&text, &mut bytes).map_err(serde::de::Error::custom)?;
        Ok(Self(bytes))
    }
}

/// Public part of a local identity. Safe to show, log and serialize.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicIdentity {
    pub account_id: AccountId,
    pub device_id: DeviceId,
    pub device_certificate: DeviceCertificate,
}

impl PublicIdentity {
    pub fn verify(&self) -> Result<()> {
        self.device_certificate.verify(&self.account_id, &self.device_id)
    }

    /// Wire form carried in envelopes and contact cards.
    pub fn to_device_identity(&self) -> DeviceIdentity {
        DeviceIdentity {
            account: *self.account_id.as_bytes(),
            device: *self.device_id.as_bytes(),
            certificate: SignatureBytes(self.device_certificate.as_bytes().to_vec()),
        }
    }
}

/// Keys of the local account and device, derived from an [`IdentitySecret`].
pub struct LocalIdentity {
    // Kept for signing device certificates and, later, account-level events.
    #[allow(dead_code)]
    account_key: SigningKey,
    // Device, mailbox and inbox keys are used by the delivery service, which
    // the engine starts in the next stage.
    #[allow(dead_code)]
    device_key: SigningKey,
    #[allow(dead_code)]
    mailbox_key: SigningKey,
    #[allow(dead_code)]
    inbox: InboxKey,
    storage_key: Zeroizing<[u8; 32]>,
    public: PublicIdentity,
}

impl LocalIdentity {
    pub fn from_secret(secret: &IdentitySecret) -> Self {
        let account_key = derive_signing_key(ACCOUNT_SIGNING_CONTEXT, &secret.account_seed);
        let device_key = derive_signing_key(DEVICE_SIGNING_CONTEXT, &secret.device_seed);
        let storage_key = Zeroizing::new(blake3::derive_key(LOCAL_STORAGE_CONTEXT, secret.device_seed.as_ref()));
        let mailbox_key = derive_signing_key(DEVICE_MAILBOX_CONTEXT, &secret.device_seed);
        let inbox_ikm = Zeroizing::new(blake3::derive_key(DEVICE_INBOX_CONTEXT, secret.device_seed.as_ref()));
        let inbox = InboxKey::derive(&inbox_ikm);

        let account_id = AccountId::from_bytes(account_key.verifying_key().to_bytes());
        let device_id = DeviceId::from_bytes(device_key.verifying_key().to_bytes());
        let signature = account_key.sign(&DeviceCertificate::signed_bytes(&account_id, &device_id));

        Self {
            account_key,
            device_key,
            mailbox_key,
            inbox,
            storage_key,
            public: PublicIdentity {
                account_id,
                device_id,
                device_certificate: DeviceCertificate(signature.to_bytes()),
            },
        }
    }

    pub fn public(&self) -> &PublicIdentity {
        &self.public
    }

    pub(crate) fn storage_key(&self) -> &[u8; 32] {
        &self.storage_key
    }

    /// Signs envelopes and invitations.
    #[allow(dead_code)]
    pub(crate) fn device_key(&self) -> &SigningKey {
        &self.device_key
    }

    /// Owns this device's mailbox on the node.
    #[allow(dead_code)]
    pub(crate) fn mailbox_key(&self) -> &SigningKey {
        &self.mailbox_key
    }

    /// Decrypts envelopes addressed to this device.
    #[allow(dead_code)]
    pub(crate) fn inbox(&self) -> &InboxKey {
        &self.inbox
    }
}

impl fmt::Debug for LocalIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalIdentity")
            .field("public", &self.public)
            .finish_non_exhaustive()
    }
}

fn derive_signing_key(context: &str, seed: &[u8; SEED_LEN]) -> SigningKey {
    let mut key_bytes = blake3::derive_key(context, seed);
    let key = SigningKey::from_bytes(&key_bytes);
    key_bytes.zeroize();
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_secret() -> IdentitySecret {
        let mut bytes = vec![SECRET_VERSION];
        bytes.extend_from_slice(&[7u8; 32]);
        bytes.extend_from_slice(&[9u8; 32]);
        IdentitySecret::from_bytes(&bytes).unwrap()
    }

    #[test]
    fn secret_round_trips() {
        let secret = IdentitySecret::generate().unwrap();
        let bytes = secret.to_bytes();
        assert_eq!(bytes.len(), IDENTITY_SECRET_LEN);
        let parsed = IdentitySecret::from_bytes(&bytes).unwrap();
        assert_eq!(
            LocalIdentity::from_secret(&secret).public(),
            LocalIdentity::from_secret(&parsed).public()
        );
    }

    #[test]
    fn rejects_malformed_secret() {
        let good = fixed_secret().to_bytes();
        assert!(IdentitySecret::from_bytes(&good[..good.len() - 1]).is_err());
        let mut wrong_version = good.to_vec();
        wrong_version[0] = 2;
        assert!(IdentitySecret::from_bytes(&wrong_version).is_err());
        assert!(IdentitySecret::from_bytes(&[]).is_err());
    }

    #[test]
    fn derivation_is_deterministic_and_separated() {
        let a = LocalIdentity::from_secret(&fixed_secret());
        let b = LocalIdentity::from_secret(&fixed_secret());
        assert_eq!(a.public(), b.public());
        // Account and device keys come from different seeds and contexts.
        assert_ne!(a.public().account_id.as_bytes(), a.public().device_id.as_bytes());
        assert_ne!(a.storage_key(), a.public().device_id.as_bytes());
    }

    #[test]
    fn device_certificate_verifies_only_for_its_pair() {
        let identity = LocalIdentity::from_secret(&fixed_secret());
        let public = identity.public().clone();
        public.verify().unwrap();

        let other = LocalIdentity::from_secret(&IdentitySecret::generate().unwrap());
        let swapped = PublicIdentity {
            device_id: other.public().device_id,
            ..public
        };
        assert!(swapped.verify().is_err());
    }

    #[test]
    fn wire_identity_verifies_with_the_protocol_crate() {
        let identity = LocalIdentity::from_secret(&fixed_secret());
        let wire = identity.public().to_device_identity();
        wire.verify().unwrap();
        // Device, mailbox and inbox keys are distinct.
        let device = identity.device_key().verifying_key().to_bytes();
        assert_ne!(device, identity.mailbox_key().verifying_key().to_bytes());
        assert_ne!(device, identity.inbox().public());
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let secret = fixed_secret();
        assert_eq!(format!("{secret:?}"), "IdentitySecret([redacted])");
        let identity = LocalIdentity::from_secret(&secret);
        let debug = format!("{identity:?}");
        assert!(!debug.contains(&hex::encode(identity.storage_key())));
    }

    #[test]
    fn public_identity_json_shape() {
        let public = LocalIdentity::from_secret(&fixed_secret()).public().clone();
        let value = serde_json::to_value(&public).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 3);
        assert_eq!(object["account_id"].as_str().unwrap().len(), 64);
        assert_eq!(object["device_certificate"].as_str().unwrap().len(), 128);
        let parsed: PublicIdentity = serde_json::from_value(value).unwrap();
        parsed.verify().unwrap();
    }
}
