use serde::{Deserialize, Serialize};

use super::{AccountId, ConversationId, DeviceId, MessageId};
use crate::error::{Error, Result};
use crate::limits::MAX_TEXT_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    /// Notes the account keeps for itself; never leaves the device yet.
    SavedMessages,
    Direct,
    /// Every member can publish. Copies are sealed separately for each member.
    Group,
    /// Only the creator publishes. Delivery is the same pairwise fan-out.
    Channel,
}

impl ConversationKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ConversationKind::SavedMessages => "saved_messages",
            ConversationKind::Direct => "direct",
            ConversationKind::Group => "group",
            ConversationKind::Channel => "channel",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "saved_messages" => Ok(ConversationKind::SavedMessages),
            "direct" => Ok(ConversationKind::Direct),
            "group" => Ok(ConversationKind::Group),
            "channel" => Ok(ConversationKind::Channel),
            _ => Err(Error::Corrupted("unknown conversation kind")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub kind: ConversationKind,
    pub created_at_ms: i64,
    pub last_message: Option<Message>,
    pub contact: Option<super::Contact>,
    /// Set for a group or a channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// False for a channel subscriber. Absent means this device may publish.
    #[serde(default = "default_can_post", skip_serializing_if = "is_true")]
    pub can_post: bool,
}

fn default_can_post() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageBody {
    Text {
        text: String,
    },
    /// Author removed the text. The plaintext is not kept.
    Deleted,
    /// Recorded voice note. The PCM WAV itself stays in `voice_notes`, not in
    /// this body, so a history page does not carry the audio.
    VoiceNote {
        duration_ms: u32,
        /// Peak amplitude of each slice, 0–255, at most 48 bars.
        waveform: Vec<u8>,
    },
}

/// Delivery state. Later stages add outbox, mailbox and recipient states;
/// they must stay distinguishable instead of collapsing into "sent".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageState {
    /// Durably committed to local storage. Nothing was sent to the network.
    SavedLocally,
    Queued,
    Mailbox,
    Delivered,
    Received,
}

impl MessageState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            MessageState::SavedLocally => "saved_locally",
            MessageState::Queued => "queued",
            MessageState::Mailbox => "mailbox",
            MessageState::Delivered => "delivered",
            MessageState::Received => "received",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "saved_locally" => Ok(MessageState::SavedLocally),
            "queued" => Ok(MessageState::Queued),
            "mailbox" => Ok(MessageState::Mailbox),
            "delivered" => Ok(MessageState::Delivered),
            "received" => Ok(MessageState::Received),
            _ => Err(Error::Corrupted("unknown message state")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    /// Local, strictly increasing position. Used for ordering and paging on
    /// this device only; it is not a network-wide order.
    pub seq: u64,
    pub author_account: AccountId,
    pub author_device: DeviceId,
    /// Wall-clock time of the author device; display only.
    pub created_at_ms: i64,
    pub body: MessageBody,
    pub state: MessageState,
    /// Zero is the original text. Each accepted edit or delete adds one.
    #[serde(default, skip_serializing_if = "revision_is_zero")]
    pub revision: u32,
    /// Author's wall clock of the latest edit or delete. Display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at_ms: Option<i64>,
    /// The author removed the text. Later edits are ignored.
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
}

fn revision_is_zero(value: &u32) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Trims surrounding whitespace and validates a text body.
pub fn normalize_text(text: &str) -> Result<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidArgument("message text is empty".into()));
    }
    if trimmed.len() > MAX_TEXT_BYTES {
        return Err(Error::InvalidArgument(format!(
            "message text exceeds {MAX_TEXT_BYTES} bytes"
        )));
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_and_validates() {
        assert_eq!(normalize_text("  hi \n").unwrap(), "hi");
        assert!(normalize_text(" \n\t ").is_err());
        assert!(normalize_text(&"a".repeat(MAX_TEXT_BYTES)).is_ok());
        assert!(normalize_text(&"a".repeat(MAX_TEXT_BYTES + 1)).is_err());
    }

    #[test]
    fn body_json_is_tagged() {
        let body = MessageBody::Text {
            text: "привет".into()
        };
        let json = serde_json::to_string(&body).unwrap();
        assert_eq!(json, r#"{"type":"text","text":"привет"}"#);
    }
}
