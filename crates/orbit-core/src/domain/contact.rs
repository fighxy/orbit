use super::{AccountId, ConversationId, DeviceId};
use serde::{Deserialize, Serialize};

/// Public contact DTO. Routing capabilities and inbox keys stay in Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub conversation_id: ConversationId,
    pub account_id: AccountId,
    pub device_id: DeviceId,
    pub display_name: String,
    /// Mutual signed-card exchange has completed.
    pub ready: bool,
    /// Latest profile picture from this contact. Empty when they have not sent one.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "super::b64")]
    pub avatar: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvitePreview {
    pub account_id: AccountId,
    pub device_id: DeviceId,
    pub display_name: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Unconfigured,
    Connecting,
    Online,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkStatus {
    pub node: Option<String>,
    pub state: ConnectionState,
    pub error: Option<String>,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self {
            node: None,
            state: ConnectionState::Unconfigured,
            error: None,
        }
    }
}
