package com.orbit.client.app

import com.orbit.sdk.IdentityStatus
import com.orbit.sdk.OrbitClient
import com.orbit.sdk.OrbitEvent
import com.orbit.sdk.OrbitSdk
import com.orbit.sdk.model.Conversation
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.Contact
import com.orbit.sdk.model.InvitePreview
import com.orbit.sdk.model.NetworkStatus
import com.orbit.sdk.model.Message
import com.orbit.sdk.model.MessageId
import com.orbit.sdk.model.MessagePage
import com.orbit.sdk.model.Profile
import com.orbit.sdk.model.Snapshot
import kotlinx.coroutines.flow.Flow

/** What the UI needs from an opened account. Implemented by [OrbitClient]. */
interface ChatBackend {
    val events: Flow<OrbitEvent>

    suspend fun snapshot(): Snapshot

    suspend fun messages(conversationId: ConversationId, beforeSeq: Long?, limit: Int): MessagePage

    suspend fun sendText(conversationId: ConversationId, text: String): Message

    suspend fun editText(conversationId: ConversationId, messageId: MessageId, text: String): Message

    suspend fun deleteText(conversationId: ConversationId, messageId: MessageId): Message

    suspend fun updateProfile(displayName: String, about: String): Profile

    suspend fun setAvatar(imageBase64: String): Profile

    suspend fun sendVoice(conversationId: ConversationId, wavBase64: String): Message

    suspend fun readVoice(messageId: MessageId): String

    suspend fun registerNode(node: String, registrationCode: String?): NetworkStatus
    suspend fun createInvite(): String
    suspend fun inspectInvite(text: String): InvitePreview
    suspend fun acceptInvite(text: String): Contact

    suspend fun createGroup(title: String, members: List<ConversationId>): Conversation

    suspend fun createChannel(title: String, members: List<ConversationId>): Conversation

    suspend fun close()
}

/** Account lifecycle. Implemented by [OrbitSdk]. */
interface AccountGateway {
    suspend fun identityStatus(): IdentityStatus

    suspend fun createIdentity(passcode: String?)

    suspend fun open(passcode: String?): ChatBackend

    suspend fun setPasscode(currentPasscode: String?, newPasscode: String)

    suspend fun removePasscode(currentPasscode: String)
}

fun OrbitSdk.asGateway(): AccountGateway = object : AccountGateway {
    override suspend fun identityStatus() = this@asGateway.identityStatus()

    override suspend fun createIdentity(passcode: String?) = this@asGateway.createIdentity(passcode)

    override suspend fun open(passcode: String?): ChatBackend = this@asGateway.open(passcode).asBackend()

    override suspend fun setPasscode(currentPasscode: String?, newPasscode: String) =
        this@asGateway.setPasscode(currentPasscode, newPasscode)

    override suspend fun removePasscode(currentPasscode: String) = this@asGateway.removePasscode(currentPasscode)
}

fun OrbitClient.asBackend(): ChatBackend = object : ChatBackend {
    override suspend fun registerNode(node: String, registrationCode: String?) = this@asBackend.registerNode(node, registrationCode)
    override suspend fun createInvite() = this@asBackend.createInvite()
    override suspend fun inspectInvite(text: String) = this@asBackend.inspectInvite(text)
    override suspend fun acceptInvite(text: String) = this@asBackend.acceptInvite(text)
    override suspend fun createGroup(title: String, members: List<ConversationId>) = this@asBackend.createGroup(title, members)
    override suspend fun createChannel(title: String, members: List<ConversationId>) = this@asBackend.createChannel(title, members)
    override val events: Flow<OrbitEvent> get() = this@asBackend.events

    override suspend fun snapshot() = this@asBackend.snapshot()

    override suspend fun messages(conversationId: ConversationId, beforeSeq: Long?, limit: Int) =
        this@asBackend.messages(conversationId, beforeSeq, limit)

    override suspend fun sendText(conversationId: ConversationId, text: String) =
        this@asBackend.sendText(conversationId, text)

    override suspend fun editText(conversationId: ConversationId, messageId: MessageId, text: String) =
        this@asBackend.editText(conversationId, messageId, text)

    override suspend fun deleteText(conversationId: ConversationId, messageId: MessageId) =
        this@asBackend.deleteText(conversationId, messageId)

    override suspend fun updateProfile(displayName: String, about: String) =
        this@asBackend.updateProfile(displayName, about)

    override suspend fun setAvatar(imageBase64: String) = this@asBackend.setAvatar(imageBase64)

    override suspend fun sendVoice(conversationId: ConversationId, wavBase64: String) =
        this@asBackend.sendVoice(conversationId, wavBase64)

    override suspend fun readVoice(messageId: MessageId) = this@asBackend.readVoice(messageId)

    override suspend fun close() = this@asBackend.close()
}
