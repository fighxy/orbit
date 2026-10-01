//! Domain model shared by storage, the engine and the client protocol.

mod contact;
mod ids;
mod message;
mod profile;

pub use contact::{ConnectionState, Contact, InvitePreview, NetworkStatus};
pub use ids::{AccountId, ConversationId, DeviceId, MessageId};
pub use message::{Conversation, ConversationKind, Message, MessageBody, MessageState, normalize_text};
pub use profile::{MAX_ABOUT_CHARS, MAX_DISPLAY_NAME_CHARS, Profile, normalize_profile};
