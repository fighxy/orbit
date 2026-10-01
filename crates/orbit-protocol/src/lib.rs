//! Orbit wire protocol shared by clients and `orbit-node`.
//!
//! This crate defines message types, canonical encoding, authentication
//! payloads and limits. It performs no I/O; transports live in
//! `orbit-transport` (client) and `orbit-node` (server).

pub mod address;
pub mod mailbox;

pub use address::NodeAddress;

/// Encodes a protocol message with postcard (compact, deterministic for a
/// given type layout).
pub fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    postcard::to_stdvec(value).map_err(|_| CodecError::Encode)
}

/// Decodes a protocol message, rejecting trailing bytes.
pub fn decode<'a, T: serde::Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, CodecError> {
    let (value, rest) = postcard::take_from_bytes(bytes).map_err(|_| CodecError::Decode)?;
    if !rest.is_empty() {
        return Err(CodecError::TrailingBytes);
    }
    Ok(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    #[error("message could not be encoded")]
    Encode,
    #[error("message could not be decoded")]
    Decode,
    #[error("message has trailing bytes")]
    TrailingBytes,
}
