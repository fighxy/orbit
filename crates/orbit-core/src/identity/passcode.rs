//! Passcode protection of the stored identity secret (app lock).
//!
//! A locked secret is sealed with a key derived from the passcode with
//! Argon2id, so reading the platform secure store alone no longer opens the
//! account. This matters most on desktops, where other processes of the same
//! user can often read keyring items. The passcode is only as strong as its
//! entropy: a short numeric code slows down, but does not prevent, an offline
//! guessing attack on a copied blob.
//!
//! Layout (version tag 0x10):
//! `tag | m_cost_kib u32le | t_cost u32le | p_cost u8 | salt[16] | nonce[24] | ciphertext`
//! The header is authenticated as associated data.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use super::IdentitySecret;
use crate::error::{Error, Result};

/// First byte of a passcode-locked identity secret.
pub const LOCKED_TAG: u8 = 0x10;
pub const MIN_PASSCODE_CHARS: usize = 4;
pub const MAX_PASSCODE_BYTES: usize = 256;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = 1 + 4 + 4 + 1 + SALT_LEN + NONCE_LEN;

/// Argon2id cost used for new locks: 64 MiB, 3 passes, 1 lane.
const DEFAULT_COST: KdfCost = KdfCost {
    m_cost_kib: 64 * 1024,
    t_cost: 3,
    p_cost: 1,
};

// Bounds accepted when unlocking, so a crafted blob cannot demand
// unbounded memory or time.
const M_COST_RANGE: std::ops::RangeInclusive<u32> = 8 * 1024..=1024 * 1024;
const T_COST_RANGE: std::ops::RangeInclusive<u32> = 1..=10;
const P_COST_RANGE: std::ops::RangeInclusive<u8> = 1..=4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KdfCost {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u8,
}

/// True when `blob` is a passcode-locked identity secret.
pub fn is_locked(blob: &[u8]) -> bool {
    blob.first() == Some(&LOCKED_TAG)
}

/// Seals an unlocked identity secret under `passcode`.
pub fn lock(secret: &[u8], passcode: &str) -> Result<Vec<u8>> {
    lock_with_cost(secret, passcode, DEFAULT_COST)
}

/// Opens a locked identity secret. Fails with [`Error::WrongPasscode`] when the
/// passcode does not match.
pub fn unlock(blob: &[u8], passcode: &str) -> Result<Zeroizing<Vec<u8>>> {
    if blob.len() < HEADER_LEN || !is_locked(blob) {
        return Err(Error::InvalidIdentity);
    }
    let cost = KdfCost {
        m_cost_kib: u32::from_le_bytes(blob[1..5].try_into().expect("4 bytes")),
        t_cost: u32::from_le_bytes(blob[5..9].try_into().expect("4 bytes")),
        p_cost: blob[9],
    };
    if !M_COST_RANGE.contains(&cost.m_cost_kib)
        || !T_COST_RANGE.contains(&cost.t_cost)
        || !P_COST_RANGE.contains(&cost.p_cost)
    {
        return Err(Error::InvalidIdentity);
    }
    let salt = &blob[10..10 + SALT_LEN];
    let nonce: [u8; NONCE_LEN] = blob[10 + SALT_LEN..HEADER_LEN].try_into().expect("24 bytes");
    let key = derive_key(passcode, salt, cost)?;
    let plaintext = XChaCha20Poly1305::new(&Key::from(*key))
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &blob[HEADER_LEN..],
                aad: &blob[..HEADER_LEN],
            },
        )
        .map_err(|_| Error::WrongPasscode)?;
    let plaintext = Zeroizing::new(plaintext);
    IdentitySecret::from_bytes(&plaintext)?;
    Ok(plaintext)
}

pub(crate) fn lock_with_cost(secret: &[u8], passcode: &str, cost: KdfCost) -> Result<Vec<u8>> {
    validate_passcode(passcode)?;
    // Only plain secrets can be locked; this also rejects double locking.
    IdentitySecret::from_bytes(secret)?;

    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut salt).map_err(|_| Error::Random)?;
    getrandom::fill(&mut nonce).map_err(|_| Error::Random)?;

    let mut blob = Vec::with_capacity(HEADER_LEN + secret.len() + 16);
    blob.push(LOCKED_TAG);
    blob.extend_from_slice(&cost.m_cost_kib.to_le_bytes());
    blob.extend_from_slice(&cost.t_cost.to_le_bytes());
    blob.push(cost.p_cost);
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce);

    let key = derive_key(passcode, &salt, cost)?;
    let ciphertext = XChaCha20Poly1305::new(&Key::from(*key))
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: secret,
                aad: &blob,
            },
        )
        .map_err(|_| Error::Internal("identity encryption failed"))?;
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

fn validate_passcode(passcode: &str) -> Result<()> {
    if passcode.chars().count() < MIN_PASSCODE_CHARS {
        return Err(Error::InvalidArgument(format!(
            "passcode must have at least {MIN_PASSCODE_CHARS} characters"
        )));
    }
    if passcode.len() > MAX_PASSCODE_BYTES {
        return Err(Error::InvalidArgument(format!(
            "passcode must not exceed {MAX_PASSCODE_BYTES} bytes"
        )));
    }
    Ok(())
}

fn derive_key(passcode: &str, salt: &[u8], cost: KdfCost) -> Result<Zeroizing<[u8; 32]>> {
    if passcode.len() > MAX_PASSCODE_BYTES {
        return Err(Error::WrongPasscode);
    }
    let params = Params::new(cost.m_cost_kib, cost.t_cost, u32::from(cost.p_cost), Some(32))
        .map_err(|_| Error::InvalidIdentity)?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passcode.as_bytes(), salt, key.as_mut())
        .map_err(|_| Error::Internal("passcode key derivation failed"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimum accepted cost keeps tests fast.
    const TEST_COST: KdfCost = KdfCost {
        m_cost_kib: 8 * 1024,
        t_cost: 1,
        p_cost: 1,
    };

    fn secret() -> Vec<u8> {
        IdentitySecret::generate().unwrap().to_bytes().to_vec()
    }

    #[test]
    fn lock_unlock_round_trip() {
        let secret = secret();
        let blob = lock_with_cost(&secret, "correct horse", TEST_COST).unwrap();
        assert!(is_locked(&blob));
        assert!(!is_locked(&secret));
        assert_eq!(unlock(&blob, "correct horse").unwrap().as_slice(), secret.as_slice());
    }

    #[test]
    fn wrong_passcode_is_reported() {
        let blob = lock_with_cost(&secret(), "1234", TEST_COST).unwrap();
        assert!(matches!(unlock(&blob, "1235"), Err(Error::WrongPasscode)));
        assert!(matches!(unlock(&blob, ""), Err(Error::WrongPasscode)));
    }

    #[test]
    fn ciphertext_does_not_contain_the_secret() {
        let secret = secret();
        let blob = lock_with_cost(&secret, "passcode", TEST_COST).unwrap();
        assert!(!blob.windows(32).any(|w| w == &secret[1..33]));
    }

    #[test]
    fn header_tampering_is_detected() {
        let blob = lock_with_cost(&secret(), "passcode", TEST_COST).unwrap();
        let mut tampered = blob.clone();
        tampered[12] ^= 1; // salt byte
        assert!(matches!(unlock(&tampered, "passcode"), Err(Error::WrongPasscode)));
        let mut tampered = blob;
        tampered[5] = 2; // t_cost changes the derived key
        assert!(matches!(unlock(&tampered, "passcode"), Err(Error::WrongPasscode)));
    }

    #[test]
    fn rejects_short_passcodes_and_double_locking() {
        let secret = secret();
        assert!(matches!(lock(&secret, "123"), Err(Error::InvalidArgument(_))));
        let blob = lock_with_cost(&secret, "passcode", TEST_COST).unwrap();
        assert!(matches!(
            lock_with_cost(&blob, "passcode", TEST_COST),
            Err(Error::InvalidIdentity)
        ));
    }

    #[test]
    fn rejects_unbounded_cost_and_truncated_blobs() {
        let mut blob = lock_with_cost(&secret(), "passcode", TEST_COST).unwrap();
        blob[1..5].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(unlock(&blob, "passcode"), Err(Error::InvalidIdentity)));
        assert!(matches!(
            unlock(&[LOCKED_TAG, 1, 2], "passcode"),
            Err(Error::InvalidIdentity)
        ));
    }

    #[test]
    fn default_cost_is_within_accepted_bounds() {
        assert!(M_COST_RANGE.contains(&DEFAULT_COST.m_cost_kib));
        assert!(T_COST_RANGE.contains(&DEFAULT_COST.t_cost));
        assert!(P_COST_RANGE.contains(&DEFAULT_COST.p_cost));
    }
}
