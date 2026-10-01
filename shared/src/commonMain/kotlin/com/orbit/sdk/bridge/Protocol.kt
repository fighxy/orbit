package com.orbit.sdk.bridge

import com.orbit.sdk.model.Conversation
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.Message
import com.orbit.sdk.model.MessagePage
import com.orbit.sdk.model.Profile
import com.orbit.sdk.model.PublicIdentity
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

// JSON protocol of orbit_core::engine. Field and variant names must match the
// Rust definitions exactly; the Rust side rejects unknown command fields.

internal val ProtocolJson = Json {
    classDiscriminator = "type"
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = false
}

@Serializable
internal sealed interface WireCommand {
    @Serializable
    @SerialName("get_snapshot")
    data object GetSnapshot : WireCommand

    @Serializable
    @SerialName("list_messages")
    data class ListMessages(
        @SerialName("conversation_id") val conversationId: ConversationId,
        @SerialName("before_seq") val beforeSeq: Long? = null,
        val limit: Int,
    ) : WireCommand

    @Serializable
    @SerialName("send_text")
    data class SendText(
        @SerialName("conversation_id") val conversationId: ConversationId,
        val text: String,
    ) : WireCommand

    @Serializable
    @SerialName("update_profile")
    data class UpdateProfile(
        @SerialName("display_name") val displayName: String,
        val about: String,
    ) : WireCommand
}

@Serializable
internal sealed interface WireResult {
    @Serializable
    @SerialName("snapshot")
    data class Snapshot(
        val identity: PublicIdentity,
        val profile: Profile? = null,
        val conversations: List<Conversation>,
    ) : WireResult

    @Serializable
    @SerialName("messages")
    data class Messages(val page: MessagePage) : WireResult

    @Serializable
    @SerialName("message_saved")
    data class MessageSaved(val message: Message) : WireResult

    @Serializable
    @SerialName("profile_updated")
    data class ProfileUpdated(val profile: Profile) : WireResult
}

@Serializable
internal data class WireError(val code: String, val message: String)

@Serializable
internal sealed interface WireEvent {
    @Serializable
    @SerialName("command_succeeded")
    data class CommandSucceeded(@SerialName("request_id") val requestId: Long, val result: WireResult) : WireEvent

    @Serializable
    @SerialName("command_failed")
    data class CommandFailed(@SerialName("request_id") val requestId: Long, val error: WireError) : WireEvent

    @Serializable
    @SerialName("message_added")
    data class MessageAdded(val message: Message) : WireEvent

    @Serializable
    @SerialName("profile_changed")
    data class ProfileChanged(val profile: Profile) : WireEvent

    @Serializable
    @SerialName("resync_required")
    data object ResyncRequired : WireEvent
}

@Serializable
internal data class WireSequencedEvent(val seq: Long, val event: WireEvent)

@Serializable
internal data class WireBatch(val events: List<WireSequencedEvent>)

internal fun WireCommand.toJsonBytes(): ByteArray =
    ProtocolJson.encodeToString(WireCommand.serializer(), this).encodeToByteArray()

internal fun decodeBatch(bytes: ByteArray): WireBatch =
    ProtocolJson.decodeFromString(WireBatch.serializer(), bytes.decodeToString())
