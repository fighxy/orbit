//! Client protocol: commands sent to the engine and events it emits.
//!
//! Serialized as JSON across the FFI boundary. Every variant is tagged with a
//! snake_case `type`. Changing a field or a variant requires bumping the ABI
//! version in `orbit-ffi`.

use serde::{Deserialize, Serialize};

use crate::domain::{Contact, Conversation, ConversationId, InvitePreview, Message, NetworkStatus, Profile};
use crate::error::ErrorInfo;
use crate::identity::PublicIdentity;
use crate::storage::MessagePage;

/// Correlates a command with its single result event.
pub type RequestId = u64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    RegisterNode {
        node: String,
        #[serde(default)]
        registration_code: Option<String>,
    },
    CreateInvite,
    InspectInvite {
        text: String,
    },
    AcceptInvite {
        text: String,
    },
    /// Public identity and conversation list. Clients request it after start
    /// and after `resync_required`.
    GetSnapshot,
    /// History page in ascending order, older than `before_seq` when given.
    ListMessages {
        conversation_id: ConversationId,
        #[serde(default)]
        before_seq: Option<u64>,
        limit: u32,
    },
    /// Stores a text message. Saved messages stay on this device. A direct
    /// conversation is sealed and handed to the outbox.
    SendText {
        conversation_id: ConversationId,
        text: String,
    },
    /// Replaces the author's own text and seals the change for the contact.
    EditText {
        conversation_id: ConversationId,
        message_id: crate::domain::MessageId,
        text: String,
    },
    /// Removes the author's own text. The peer sees a deletion, not the old text.
    DeleteText {
        conversation_id: ConversationId,
        message_id: crate::domain::MessageId,
    },
    /// Pairwise group with the ready direct contacts in `members`.
    CreateGroup {
        title: String,
        members: Vec<ConversationId>,
    },
    /// Pairwise channel. Only this device can publish.
    CreateChannel {
        title: String,
        members: Vec<ConversationId>,
    },
    /// Sets the local profile. The name is required; `about` may be empty.
    UpdateProfile {
        display_name: String,
        #[serde(default)]
        about: String,
    },
    /// Sets or clears the profile picture. `image` is standard base64.
    /// Empty clears it. JPEG or PNG, at most 32 KiB.
    SetAvatar {
        image: String,
    },
    /// Sends a voice note. `wav_base64` is a 16 kHz mono 16-bit PCM WAV,
    /// at most 60 seconds. Not a live room and not a video circle.
    SendVoice {
        conversation_id: ConversationId,
        wav_base64: String,
    },
    /// Returns the stored WAV for a voice note.
    ReadVoice {
        message_id: crate::domain::MessageId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandResult {
    Snapshot {
        identity: PublicIdentity,
        /// `None` until the user sets a profile.
        profile: Option<Profile>,
        conversations: Vec<Conversation>,
        network: NetworkStatus,
    },
    NodeRegistered {
        network: NetworkStatus,
    },
    InviteCreated {
        text: String,
    },
    InviteInspected {
        preview: InvitePreview,
    },
    ContactAdded {
        contact: Contact,
    },
    Messages {
        page: MessagePage,
    },
    MessageSaved {
        message: Message,
    },
    ProfileUpdated {
        profile: Profile,
    },
    RoomCreated {
        conversation: Conversation,
    },
    Voice {
        message_id: crate::domain::MessageId,
        wav_base64: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    CommandSucceeded {
        request_id: RequestId,
        result: CommandResult,
    },
    CommandFailed {
        request_id: RequestId,
        error: ErrorInfo,
    },
    /// A message became durable in local storage.
    MessageAdded {
        message: Message,
    },
    /// The local profile changed.
    ProfileChanged {
        profile: Profile,
    },
    ContactsChanged,
    NetworkChanged {
        network: NetworkStatus,
    },
    /// Events were dropped because the client did not read them in time.
    /// The client must request a new snapshot and reload visible history.
    ResyncRequired,
}

impl Event {
    /// Results are never dropped; their number is bounded by in-flight
    /// commands. Notifications can be recovered through a snapshot.
    pub(crate) fn is_droppable(&self) -> bool {
        matches!(
            self,
            Event::MessageAdded { .. }
                | Event::ProfileChanged { .. }
                | Event::ContactsChanged
                | Event::NetworkChanged { .. }
        )
    }

    pub(crate) fn is_command_result(&self) -> bool {
        matches!(self, Event::CommandSucceeded { .. } | Event::CommandFailed { .. })
    }
}

/// Event with its position in the engine event stream. Sequence numbers
/// increase by one per emitted event; gaps mean dropped events and are always
/// followed by `resync_required`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequencedEvent {
    pub seq: u64,
    pub event: Event,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_parse_from_json() {
        let snapshot: Command = serde_json::from_str(r#"{"type":"get_snapshot"}"#).unwrap();
        assert_eq!(snapshot, Command::GetSnapshot);

        let id = "11".repeat(16);
        let list: Command = serde_json::from_str(&format!(
            r#"{{"type":"list_messages","conversation_id":"{id}","limit":20}}"#
        ))
        .unwrap();
        assert_eq!(
            list,
            Command::ListMessages {
                conversation_id: id.parse().unwrap(),
                before_seq: None,
                limit: 20
            }
        );
    }

    #[test]
    fn unknown_commands_and_fields_are_rejected() {
        assert!(serde_json::from_str::<Command>(r#"{"type":"drop_tables"}"#).is_err());
        let id = "11".repeat(16);
        assert!(
            serde_json::from_str::<Command>(&format!(
                r#"{{"type":"send_text","conversation_id":"{id}","text":"x","extra":1}}"#
            ))
            .is_err()
        );
        assert!(
            serde_json::from_str::<Command>(r#"{"type":"list_messages","conversation_id":"zz","limit":1}"#).is_err()
        );
    }

    #[test]
    fn events_serialize_with_type_tags() {
        let event = SequencedEvent {
            seq: 7,
            event: Event::ResyncRequired,
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"seq":7,"event":{"type":"resync_required"}}"#
        );
    }
}
