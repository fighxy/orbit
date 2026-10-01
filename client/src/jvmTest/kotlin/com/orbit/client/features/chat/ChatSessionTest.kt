package com.orbit.client.features.chat

import com.orbit.client.app.ChatBackend
import com.orbit.sdk.OrbitEvent
import com.orbit.sdk.model.*
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

class ChatSessionTest {
    @Test
    fun lateSendResultDoesNotUndoDeliveryReceipt() = runBlocking {
        val conversation = ConversationId("01".repeat(16))
        val message = Message(MessageId("02".repeat(16)), conversation, 1,
            AccountId("03".repeat(32)), DeviceId("04".repeat(32)), 1, MessageBody.Text("hi"), MessageState.Queued)
        val sent = CompletableDeferred<Message>()
        val events = MutableSharedFlow<OrbitEvent>()
        val backend = object : ChatBackend {
            override val events: Flow<OrbitEvent> = events
            override suspend fun snapshot() = Snapshot(PublicIdentity(message.authorAccount, message.authorDevice, "00".repeat(64)), null,
                listOf(Conversation(conversation, ConversationKind.SavedMessages, 0)))
            override suspend fun messages(conversationId: ConversationId, beforeSeq: Long?, limit: Int) = MessagePage(emptyList(), false)
            override suspend fun sendText(conversationId: ConversationId, text: String) = sent.await()
            override suspend fun updateProfile(displayName: String, about: String): Profile = error("unused")
            override suspend fun registerNode(node: String, registrationCode: String?): NetworkStatus = error("unused")
            override suspend fun createInvite(): String = error("unused")
            override suspend fun inspectInvite(text: String): InvitePreview = error("unused")
            override suspend fun acceptInvite(text: String): Contact = error("unused")
            override suspend fun close() {}
        }
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val session = ChatSession(backend, scope)
        try {
            session.start(); session.select(conversation); session.send("hi")
            events.emit(OrbitEvent.MessageAdded(message.copy(state = MessageState.Delivered)))
            withTimeout(1000) { session.chat.first { it.messages.singleOrNull()?.state == MessageState.Delivered } }
            sent.complete(message)
            withTimeout(1000) { session.chat.first { it.sending == 0 } }
            assertEquals(MessageState.Delivered, session.chat.value.messages.single().state)
            assertEquals(MessageState.Delivered, session.conversations.value.single().lastMessage?.state)
        } finally { session.close(); scope.cancel() }
    }
}
