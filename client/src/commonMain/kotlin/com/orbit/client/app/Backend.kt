package com.orbit.client.app

import com.orbit.sdk.IdentityStatus
import com.orbit.sdk.OrbitClient
import com.orbit.sdk.OrbitEvent
import com.orbit.sdk.OrbitSdk
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.Message
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

    suspend fun updateProfile(displayName: String, about: String): Profile

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
    override val events: Flow<OrbitEvent> get() = this@asBackend.events

    override suspend fun snapshot() = this@asBackend.snapshot()

    override suspend fun messages(conversationId: ConversationId, beforeSeq: Long?, limit: Int) =
        this@asBackend.messages(conversationId, beforeSeq, limit)

    override suspend fun sendText(conversationId: ConversationId, text: String) =
        this@asBackend.sendText(conversationId, text)

    override suspend fun updateProfile(displayName: String, about: String) =
        this@asBackend.updateProfile(displayName, about)

    override suspend fun close() = this@asBackend.close()
}
