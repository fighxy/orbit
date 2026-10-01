package com.orbit.sdk.bridge

import com.orbit.sdk.model.Contact
import com.orbit.sdk.model.InvitePreview
import com.orbit.sdk.model.NetworkStatus
import com.orbit.sdk.model.Conversation
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.Message
import com.orbit.sdk.model.MessageId
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
    @Serializable @SerialName("register_node")
    data class RegisterNode(val node: String, @SerialName("registration_code") val registrationCode: String? = null) : WireCommand
    @Serializable @SerialName("create_invite") data object CreateInvite : WireCommand
    @Serializable @SerialName("inspect_invite") data class InspectInvite(val text: String) : WireCommand
    @Serializable @SerialName("accept_invite") data class AcceptInvite(val text: String) : WireCommand

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
    @SerialName("edit_text")
    data class EditText(
        @SerialName("conversation_id") val conversationId: ConversationId,
        @SerialName("message_id") val messageId: MessageId,
        val text: String,
    ) : WireCommand

    @Serializable
    @SerialName("delete_text")
    data class DeleteText(
        @SerialName("conversation_id") val conversationId: ConversationId,
        @SerialName("message_id") val messageId: MessageId,
    ) : WireCommand

    @Serializable
    @SerialName("update_profile")
    data class UpdateProfile(
        @SerialName("display_name") val displayName: String,
        val about: String,
    ) : WireCommand

    @Serializable
    @SerialName("create_group")
    data class CreateGroup(
        val title: String,
        val members: List<ConversationId>,
    ) : WireCommand

    @Serializable
    @SerialName("create_channel")
    data class CreateChannel(
        val title: String,
        val members: List<ConversationId>,
    ) : WireCommand

    @Serializable
    @SerialName("set_avatar")
    data class SetAvatar(val image: String) : WireCommand

    @Serializable
    @SerialName("send_voice")
    data class SendVoice(
        @SerialName("conversation_id") val conversationId: ConversationId,
        @SerialName("wav_base64") val wavBase64: String,
    ) : WireCommand

    @Serializable
    @SerialName("read_voice")
    data class ReadVoice(@SerialName("message_id") val messageId: MessageId) : WireCommand
}

@Serializable
internal sealed interface WireResult {
    @Serializable @SerialName("node_registered") data class NodeRegistered(val network: NetworkStatus) : WireResult
    @Serializable @SerialName("invite_created") data class InviteCreated(val text: String) : WireResult
    @Serializable @SerialName("invite_inspected") data class InviteInspected(val preview: InvitePreview) : WireResult
    @Serializable @SerialName("contact_added") data class ContactAdded(val contact: Contact) : WireResult

    @Serializable
    @SerialName("snapshot")
    data class Snapshot(
        val identity: PublicIdentity,
        val profile: Profile? = null,
        val conversations: List<Conversation>,
        val network: NetworkStatus = NetworkStatus(),
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

    @Serializable
    @SerialName("room_created")
    data class RoomCreated(val conversation: Conversation) : WireResult

    @Serializable
    @SerialName("voice")
    data class Voice(
        @SerialName("message_id") val messageId: MessageId,
        @SerialName("wav_base64") val wavBase64: String,
    ) : WireResult
}

@Serializable
internal data class WireError(val code: String, val message: String)

@Serializable
internal sealed interface WireEvent {
    @Serializable @SerialName("contacts_changed") data object ContactsChanged : WireEvent
    @Serializable @SerialName("network_changed") data class NetworkChanged(val network: NetworkStatus) : WireEvent

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
