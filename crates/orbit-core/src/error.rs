//! Error type shared by the core and the FFI boundary.
//!
//! Messages must never contain key material or message plaintext: they are
//! surfaced to UI, logs and exception messages on the Kotlin side.

use serde::{Deserialize, Serialize};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("identity secret is malformed or has an unsupported version")]
    InvalidIdentity,
    #[error("account storage is already opened by another engine")]
    StorageLocked,
    #[error("account storage belongs to a different account or device")]
    IdentityMismatch,
    #[error("storage key does not match the stored data")]
    StorageKeyMismatch,
    #[error("stored data is corrupted: {0}")]
    Corrupted(&'static str),
    #[error("storage schema version {0} is not supported by this build")]
    UnsupportedStorageVersion(i64),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("storage failure: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("i/o failure: {0}")]
    Io(#[from] std::io::Error),
    #[error("operating system random number generator failed")]
    Random,
    #[error("engine is closed")]
    Closed,
    #[error("too many requests are in flight")]
    Busy,
    #[error("internal error: {0}")]
    Internal(&'static str),
    #[error("passcode is incorrect")]
    WrongPasscode,
    #[error("delivery server is not configured")]
    NetworkNotConfigured,
    #[error("invitation is invalid: {0}")]
    InvalidInvite(&'static str),
    #[error("delivery failure: {0}")]
    Network(&'static str),
}

/// Stable error codes. Values are part of the C ABI and must not be reused.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidArgument = 1,
    InvalidIdentity = 2,
    StorageLocked = 3,
    IdentityMismatch = 4,
    StorageKeyMismatch = 5,
    Corrupted = 6,
    UnsupportedStorageVersion = 7,
    NotFound = 8,
    Storage = 9,
    Io = 10,
    Random = 11,
    Closed = 12,
    Busy = 13,
    Internal = 14,
    WrongPasscode = 15,
    NetworkNotConfigured = 16,
    InvalidInvite = 17,
    Network = 18,
}

impl Error {
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::InvalidArgument(_) => ErrorCode::InvalidArgument,
            Error::InvalidIdentity => ErrorCode::InvalidIdentity,
            Error::StorageLocked => ErrorCode::StorageLocked,
            Error::IdentityMismatch => ErrorCode::IdentityMismatch,
            Error::StorageKeyMismatch => ErrorCode::StorageKeyMismatch,
            Error::Corrupted(_) => ErrorCode::Corrupted,
            Error::UnsupportedStorageVersion(_) => ErrorCode::UnsupportedStorageVersion,
            Error::NotFound(_) => ErrorCode::NotFound,
            Error::Storage(_) => ErrorCode::Storage,
            Error::Io(_) => ErrorCode::Io,
            Error::Random => ErrorCode::Random,
            Error::Closed => ErrorCode::Closed,
            Error::Busy => ErrorCode::Busy,
            Error::Internal(_) => ErrorCode::Internal,
            Error::WrongPasscode => ErrorCode::WrongPasscode,
            Error::NetworkNotConfigured => ErrorCode::NetworkNotConfigured,
            Error::InvalidInvite(_) => ErrorCode::InvalidInvite,
            Error::Network(_) => ErrorCode::Network,
        }
    }

    pub fn info(&self) -> ErrorInfo {
        ErrorInfo {
            code: self.code(),
            message: self.to_string(),
        }
    }
}

/// Serializable error description delivered to clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: ErrorCode,
    pub message: String,
}
