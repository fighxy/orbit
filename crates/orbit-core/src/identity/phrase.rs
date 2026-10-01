//! 24-word recovery phrase.
//!
//! The encoding is BIP39 English: 256 bits of entropy plus an 8-bit SHA-256
//! checksum, as 24 words. The phrase is the account. It does not contain the
//! message database.

use std::sync::OnceLock;

use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::error::{Error, Result};

const WORD_COUNT: usize = 24;

fn dictionary() -> &'static [&'static str] {
    static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| {
        include_str!("bip39_english.txt")
            .lines()
            .filter(|line| !line.is_empty())
            .collect()
    })
}

/// Encodes 32 entropy bytes as 24 lowercase words separated by a single space.
pub(super) fn encode(entropy: &[u8; 32]) -> String {
    let checksum = Sha256::digest(entropy);
    let mut bits = [0u8; 33];
    bits[..32].copy_from_slice(entropy);
    bits[32] = checksum[0];
    let words = dictionary();
    let mut phrase = String::new();
    for index in 0..WORD_COUNT {
        if index > 0 {
            phrase.push(' ');
        }
        phrase.push_str(words[read_bits(&bits, index * 11, 11)]);
    }
    bits.zeroize();
    phrase
}

/// Decodes a 24-word phrase. Extra whitespace is ignored. Words are matched
/// in lowercase. A bad checksum or an unknown word is [`Error::InvalidIdentity`].
pub(super) fn decode(text: &str) -> Result<[u8; 32]> {
    let words = dictionary();
    if words.len() != 2048 {
        return Err(Error::Internal("recovery wordlist"));
    }
    let parsed: Vec<&str> = text.split_whitespace().collect();
    if parsed.len() != WORD_COUNT {
        return Err(Error::InvalidIdentity);
    }
    let mut indexes = [0usize; WORD_COUNT];
    for (slot, word) in parsed.iter().enumerate() {
        let key = word.to_ascii_lowercase();
        indexes[slot] = words.binary_search(&key.as_str()).map_err(|_| Error::InvalidIdentity)?;
    }
    let mut bits = [0u8; 33];
    for (index, value) in indexes.iter().enumerate() {
        write_bits(&mut bits, index * 11, 11, *value);
    }
    let mut entropy = [0u8; 32];
    entropy.copy_from_slice(&bits[..32]);
    let checksum = Sha256::digest(entropy);
    if bits[32] != checksum[0] {
        return Err(Error::InvalidIdentity);
    }
    Ok(entropy)
}

fn read_bits(bytes: &[u8], start: usize, len: usize) -> usize {
    let mut value = 0usize;
    for offset in 0..len {
        let position = start + offset;
        let bit = (bytes[position / 8] >> (7 - (position % 8))) & 1;
        value = (value << 1) | usize::from(bit);
    }
    value
}

fn write_bits(bytes: &mut [u8], start: usize, len: usize, value: usize) {
    for offset in 0..len {
        let position = start + offset;
        let bit = ((value >> (len - 1 - offset)) & 1) as u8;
        let shift = 7 - (position % 8);
        bytes[position / 8] |= bit << shift;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_entropy_matches_the_bip39_vector() {
        let phrase = encode(&[0u8; 32]);
        assert_eq!(
            phrase,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art"
        );
        assert_eq!(decode(&phrase).unwrap(), [0u8; 32]);
    }

    #[test]
    fn rejects_a_modified_checksum_and_unknown_words() {
        let phrase = encode(&[0u8; 32]);
        let mut words: Vec<&str> = phrase.split(' ').collect();
        words[23] = "abandon";
        assert!(decode(&words.join(" ")).is_err());
        words[0] = "notaword";
        assert!(decode(&words.join(" ")).is_err());
        assert!(decode("abandon abandon").is_err());
    }
}
