package com.orbit.sdk.model

import kotlin.jvm.JvmInline
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** Account identity: hex of the account Ed25519 public key. */
@Serializable
@JvmInline
value class AccountId(val hex: String)

/** Device identity: hex of the device Ed25519 public key. */
@Serializable
@JvmInline
value class DeviceId(val hex: String)

@Serializable
@JvmInline
value class ConversationId(val hex: String)

@Serializable
@JvmInline
value class MessageId(val hex: String)

/**
 * Public identity of the local account on this device. Contains no secrets;
 * the secret stays in [com.orbit.sdk.platform.SecretStore] and the engine.
 */
@Serializable
data class PublicIdentity(
    @SerialName("account_id") val accountId: AccountId,
    @SerialName("device_id") val deviceId: DeviceId,
    @SerialName("device_certificate") val deviceCertificate: String,
) {
    /** Short human-comparable form of the account key, e.g. `3F2A 91C0 7B44 E210`. */
    val accountFingerprint: String
        get() = accountId.hex.take(16).uppercase().chunked(4).joinToString(" ")
}

@Serializable
enum class ConversationKind {
    @SerialName("saved_messages")
    SavedMessages,
    @SerialName("direct") Direct,
}

@Serializable
data class Conversation(
    val id: ConversationId,
    val kind: ConversationKind,
    @SerialName("created_at_ms") val createdAtMs: Long,
    @SerialName("last_message") val lastMessage: Message? = null,
    val contact: Contact? = null,
)

@Serializable
sealed interface MessageBody {
    @Serializable
    @SerialName("text")
    data class Text(val text: String) : MessageBody
}

/** Delivery state. Each value describes exactly what is known to have happened. */
@Serializable
enum class MessageState {
    /** Durably stored on this device; nothing was sent to the network. */
    @SerialName("saved_locally")
    SavedLocally,
    @SerialName("queued") Queued,
    @SerialName("mailbox") Mailbox,
    @SerialName("delivered") Delivered,
    @SerialName("received") Received,
}

@Serializable
data class Message(
    val id: MessageId,
    @SerialName("conversation_id") val conversationId: ConversationId,
    /** Local order on this device; not a network-wide order. */
    val seq: Long,
    @SerialName("author_account") val authorAccount: AccountId,
    @SerialName("author_device") val authorDevice: DeviceId,
    /** Author wall clock, for display only. */
    @SerialName("created_at_ms") val createdAtMs: Long,
    val body: MessageBody,
    val state: MessageState,
)

/** History page in ascending order. */
@Serializable
data class MessagePage(
    val messages: List<Message>,
    @SerialName("has_more") val hasMore: Boolean,
)

/** Local profile; shared with contacts once contact exchange exists. */
@Serializable
data class Profile(
    @SerialName("display_name") val displayName: String,
    val about: String,
    @SerialName("updated_at_ms") val updatedAtMs: Long,
)

data class Snapshot(
    val identity: PublicIdentity,
    /** `null` until the user sets a profile. */
    val profile: Profile?,
    val conversations: List<Conversation>,
    val network: NetworkStatus = NetworkStatus(),
)

@Serializable
data class Contact(
    @SerialName("conversation_id") val conversationId: ConversationId,
    @SerialName("account_id") val accountId: AccountId,
    @SerialName("device_id") val deviceId: DeviceId,
    @SerialName("display_name") val displayName: String,
    val ready: Boolean,
)

@Serializable
data class InvitePreview(
    @SerialName("account_id") val accountId: AccountId,
    @SerialName("device_id") val deviceId: DeviceId,
    @SerialName("display_name") val displayName: String,
    @SerialName("expires_at_ms") val expiresAtMs: Long,
)

@Serializable
enum class ConnectionState {
    @SerialName("unconfigured") Unconfigured,
    @SerialName("connecting") Connecting,
    @SerialName("online") Online,
    @SerialName("offline") Offline,
}

@Serializable
data class NetworkStatus(
    val node: String? = null,
    val state: ConnectionState = ConnectionState.Unconfigured,
    val error: String? = null,
)
