//! Orbit infrastructure node.
//!
//! The first service is the mailbox: store-and-forward of opaque envelopes
//! for recipients that are offline, with TTL, quotas, owner authentication
//! and deposit tokens. One process can later host relay and push functions
//! as separate capabilities.

pub mod config;
pub mod host;
pub mod server;
pub mod store;

pub use config::{Config, MailboxConfig};
pub use server::{Node, NodeError};
