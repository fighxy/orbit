//! Domain model shared by storage, the engine and the client protocol.

mod ids;
mod message;

pub use ids::{AccountId, ConversationId, DeviceId, MessageId};
pub use message::{Conversation, ConversationKind, Message, MessageBody, MessageState, normalize_text};
