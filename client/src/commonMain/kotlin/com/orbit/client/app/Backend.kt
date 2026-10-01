package com.orbit.client.app

import com.orbit.sdk.OrbitClient
import com.orbit.sdk.OrbitEvent
import com.orbit.sdk.OrbitSdk
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.Message
import com.orbit.sdk.model.MessagePage
import com.orbit.sdk.model.Snapshot
import kotlinx.coroutines.flow.Flow

/** What the UI needs from an opened account. Implemented by [OrbitClient]. */
interface ChatBackend {
    val events: Flow<OrbitEvent>

    suspend fun snapshot(): Snapshot

    suspend fun messages(conversationId: ConversationId, beforeSeq: Long?, limit: Int): MessagePage

    suspend fun sendText(conversationId: ConversationId, text: String): Message

    suspend fun close()
}

/** Account lifecycle. Implemented by [OrbitSdk]. */
interface AccountGateway {
    suspend fun identityExists(): Boolean

    suspend fun createIdentity()

    suspend fun open(): ChatBackend
}

fun OrbitSdk.asGateway(): AccountGateway = object : AccountGateway {
    override suspend fun identityExists() = this@asGateway.identityExists()

    override suspend fun createIdentity() = this@asGateway.createIdentity()

    override suspend fun open(): ChatBackend = this@asGateway.open().asBackend()
}

fun OrbitClient.asBackend(): ChatBackend = object : ChatBackend {
    override val events: Flow<OrbitEvent> get() = this@asBackend.events

    override suspend fun snapshot() = this@asBackend.snapshot()

    override suspend fun messages(conversationId: ConversationId, beforeSeq: Long?, limit: Int) =
        this@asBackend.messages(conversationId, beforeSeq, limit)

    override suspend fun sendText(conversationId: ConversationId, text: String) =
        this@asBackend.sendText(conversationId, text)

    override suspend fun close() = this@asBackend.close()
}
