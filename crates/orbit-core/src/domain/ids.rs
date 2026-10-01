//! Strongly typed identifiers. All IDs cross the FFI boundary as lowercase hex.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use crate::error::{Error, Result};

macro_rules! byte_id {
    ($(#[$meta:meta])* $name:ident, $len:literal, $label:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; $len]);

        impl $name {
            pub const LEN: usize = $len;

            pub const fn from_bytes(bytes: [u8; $len]) -> Self {
                Self(bytes)
            }

            pub fn from_slice(bytes: &[u8]) -> Result<Self> {
                let array: [u8; $len] = bytes
                    .try_into()
                    .map_err(|_| Error::InvalidArgument(format!("{} must be {} bytes", $label, $len)))?;
                Ok(Self(array))
            }

            pub const fn as_bytes(&self) -> &[u8; $len] {
                &self.0
            }

            pub fn to_hex(&self) -> String {
                hex::encode(self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_hex())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.to_hex())
            }
        }

        impl FromStr for $name {
            type Err = Error;

            fn from_str(s: &str) -> Result<Self> {
                if s.len() != $len * 2 || s.bytes().any(|b| b.is_ascii_uppercase()) {
                    return Err(Error::InvalidArgument(format!(
                        "{} must be {} lowercase hex characters",
                        $label,
                        $len * 2
                    )));
                }
                let mut bytes = [0u8; $len];
                hex::decode_to_slice(s, &mut bytes)
                    .map_err(|_| Error::InvalidArgument(format!("{} is not valid hex", $label)))?;
                Ok(Self(bytes))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_hex())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
                struct IdVisitor;

                impl Visitor<'_> for IdVisitor {
                    type Value = $name;

                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        write!(f, "{} as {} lowercase hex characters", $label, $len * 2)
                    }

                    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<$name, E> {
                        v.parse().map_err(|_| E::invalid_value(de::Unexpected::Str(v), &self))
                    }
                }

                deserializer.deserialize_str(IdVisitor)
            }
        }
    };
}

macro_rules! random_id {
    ($name:ident) => {
        impl $name {
            pub fn random() -> Result<Self> {
                let mut bytes = [0u8; Self::LEN];
                getrandom::fill(&mut bytes).map_err(|_| Error::Random)?;
                Ok(Self(bytes))
            }
        }
    };
}

byte_id!(
    /// Account identity: the Ed25519 public key of the account signing key.
    AccountId, 32, "account id"
);
byte_id!(
    /// Device identity: the Ed25519 public key of one device's signing key.
    DeviceId, 32, "device id"
);
byte_id!(
    /// Conversation scope identifier.
    ConversationId, 16, "conversation id"
);
byte_id!(
    /// Stable message identifier used for dedup, replies, edits and deletes.
    MessageId, 16, "message id"
);

random_id!(ConversationId);
random_id!(MessageId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let id = MessageId::from_bytes([0xab; 16]);
        let text = id.to_string();
        assert_eq!(text, "ab".repeat(16));
        assert_eq!(text.parse::<MessageId>().unwrap(), id);
    }

    #[test]
    fn rejects_wrong_length_and_uppercase() {
        assert!("abcd".parse::<MessageId>().is_err());
        assert!("AB".repeat(16).parse::<MessageId>().is_err());
        assert!("zz".repeat(16).parse::<MessageId>().is_err());
    }

    #[test]
    fn serde_uses_hex_strings() {
        let id = ConversationId::from_bytes([1; 16]);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", "01".repeat(16)));
        assert_eq!(serde_json::from_str::<ConversationId>(&json).unwrap(), id);
        assert!(serde_json::from_str::<ConversationId>("\"01\"").is_err());
    }

    #[test]
    fn random_ids_differ() {
        assert_ne!(MessageId::random().unwrap(), MessageId::random().unwrap());
    }
}
