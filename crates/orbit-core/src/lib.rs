//! Orbit core: identity, local storage, domain rules and the engine facade.
//!
//! The engine is the only owner of message storage. Platform clients talk to it
//! through `orbit-ffi` using versioned JSON commands and events; they never
//! open the database or hold key material other than the opaque identity
//! secret kept in the platform secure store.

pub mod domain;
pub mod engine;
pub mod error;
pub mod identity;
pub mod limits;
pub mod storage;

pub use engine::{Command, CommandResult, Engine, EngineConfig, Event, RequestId, SequencedEvent};
pub use error::{Error, ErrorCode, ErrorInfo, Result};
pub use identity::{IdentitySecret, LocalIdentity, PublicIdentity};
