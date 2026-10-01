package com.orbit.sdk

import com.orbit.sdk.bridge.NativeEngine
import com.orbit.sdk.bridge.OrbitErrorCode
import com.orbit.sdk.bridge.OrbitNativeException
import com.orbit.sdk.bridge.WireCommand
import com.orbit.sdk.bridge.WireEvent
import com.orbit.sdk.bridge.WireResult
import com.orbit.sdk.bridge.decodeBatch
import com.orbit.sdk.bridge.toJsonBytes
import com.orbit.sdk.model.Contact
import com.orbit.sdk.model.Conversation
import com.orbit.sdk.model.InvitePreview
import com.orbit.sdk.model.NetworkStatus
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.Message
import com.orbit.sdk.model.MessageId
import com.orbit.sdk.model.MessagePage
import com.orbit.sdk.model.Profile
import com.orbit.sdk.model.Snapshot
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/** Notifications from the engine that are not answers to a call. */
sealed interface OrbitEvent {
    data object ContactsChanged : OrbitEvent
    data class NetworkChanged(val network: NetworkStatus) : OrbitEvent

    /** A message became durable locally. Upsert by [Message.id]. */
    data class MessageAdded(val message: Message) : OrbitEvent

    /** The local profile changed. */
    data class ProfileChanged(val profile: Profile) : OrbitEvent

    /** Events were dropped; reload the snapshot and visible history. */
    data object ResyncRequired : OrbitEvent
}

sealed interface ClientState {
    data object Running : ClientState

    data object Closed : ClientState

    /** The event stream stopped unexpectedly; the client must be reopened. */
    data class Failed(val error: OrbitException) : ClientState
}

/** Failure of an engine call, with the engine error code. */
class OrbitException(val code: OrbitErrorCode, message: String) : Exception(message)

/**
 * Coroutine facade over one opened engine.
 *
 * Collect [events] before calling [snapshot] so that no message announced
 * between the snapshot and the subscription is missed.
 */
class OrbitClient internal constructor(
    private val engine: NativeEngine,
    private val ioDispatcher: CoroutineDispatcher,
) {
    private val scope = CoroutineScope(SupervisorJob() + ioDispatcher)
    private val lock = Mutex()
    private val pending = HashMap<Long, CompletableDeferred<WireResult>>()
    private var terminalError: OrbitException? = null

    private val mutableEvents = MutableSharedFlow<OrbitEvent>(extraBufferCapacity = 64)
    val events: SharedFlow<OrbitEvent> = mutableEvents.asSharedFlow()

    private val mutableState = MutableStateFlow<ClientState>(ClientState.Running)
    val state: StateFlow<ClientState> = mutableState.asStateFlow()

    private val eventLoop = scope.launch { runEventLoop() }

    suspend fun snapshot(): Snapshot {
        val result = call(WireCommand.GetSnapshot) as WireResult.Snapshot
        return Snapshot(result.identity, result.profile, result.conversations, result.network)
    }

    suspend fun registerNode(node: String, registrationCode: String? = null): NetworkStatus =
        (call(WireCommand.RegisterNode(node, registrationCode)) as WireResult.NodeRegistered).network

    suspend fun createInvite(): String = (call(WireCommand.CreateInvite) as WireResult.InviteCreated).text

    suspend fun inspectInvite(text: String): InvitePreview =
        (call(WireCommand.InspectInvite(text)) as WireResult.InviteInspected).preview

    suspend fun acceptInvite(text: String): Contact =
        (call(WireCommand.AcceptInvite(text)) as WireResult.ContactAdded).contact

    /** Pairwise group. `members` are ready direct conversations. */
    suspend fun createGroup(title: String, members: List<ConversationId>): Conversation =
        (call(WireCommand.CreateGroup(title, members)) as WireResult.RoomCreated).conversation

    /** Pairwise channel. Only this device can publish. */
    suspend fun createChannel(title: String, members: List<ConversationId>): Conversation =
        (call(WireCommand.CreateChannel(title, members)) as WireResult.RoomCreated).conversation

    /** Messages older than [beforeSeq] (newest when null), oldest first. */
    suspend fun messages(conversationId: ConversationId, beforeSeq: Long? = null, limit: Int = 50): MessagePage {
        val result = call(WireCommand.ListMessages(conversationId, beforeSeq, limit)) as WireResult.Messages
        return result.page
    }

    /** Stores a text message; returns once it is durable. */
    suspend fun sendText(conversationId: ConversationId, text: String): Message {
        val result = call(WireCommand.SendText(conversationId, text)) as WireResult.MessageSaved
        return result.message
    }

    /** Replaces the author's own text. The contact receives the same change. */
    suspend fun editText(conversationId: ConversationId, messageId: MessageId, text: String): Message {
        val result = call(WireCommand.EditText(conversationId, messageId, text)) as WireResult.MessageSaved
        return result.message
    }

    /** Removes the author's own text. The contact sees a deletion. */
    suspend fun deleteText(conversationId: ConversationId, messageId: MessageId): Message {
        val result = call(WireCommand.DeleteText(conversationId, messageId)) as WireResult.MessageSaved
        return result.message
    }

    /** Sets the local profile; the name is required, [about] may be empty. */
    suspend fun updateProfile(displayName: String, about: String): Profile {
        val result = call(WireCommand.UpdateProfile(displayName, about)) as WireResult.ProfileUpdated
        return result.profile
    }

    /**
     * Sets the profile picture. [imageBase64] is standard base64 of a JPEG or PNG,
     * at most 32 KiB. An empty string clears it.
     */
    suspend fun setAvatar(imageBase64: String): Profile {
        val result = call(WireCommand.SetAvatar(imageBase64)) as WireResult.ProfileUpdated
        return result.profile
    }

    /** Sends a 16 kHz mono 16-bit PCM WAV, at most 60 seconds. */
    suspend fun sendVoice(conversationId: ConversationId, wavBase64: String): Message {
        val result = call(WireCommand.SendVoice(conversationId, wavBase64)) as WireResult.MessageSaved
        return result.message
    }

    /** Stored WAV for a voice note, standard base64. */
    suspend fun readVoice(messageId: MessageId): String {
        val result = call(WireCommand.ReadVoice(messageId)) as WireResult.Voice
        return result.wavBase64
    }

    /** Closes the engine and fails pending calls. Idempotent. */
    suspend fun close() {
        withContext(NonCancellable + ioDispatcher) {
            engine.close()
            eventLoop.join()
        }
        scope.cancel()
    }

    private suspend fun call(command: WireCommand): WireResult {
        val bytes = command.toJsonBytes()
        val result = CompletableDeferred<WireResult>()
        // Submitting under the lock guarantees the event loop cannot see the
        // result before the request is registered.
        val requestId = lock.withLock {
            terminalError?.let { throw it }
            val id = try {
                engine.submit(bytes)
            } catch (e: OrbitNativeException) {
                throw e.toOrbitException()
            }
            pending[id] = result
            id
        }
        try {
            return result.await()
        } catch (e: CancellationException) {
            withContext(NonCancellable) { lock.withLock { pending.remove(requestId) } }
            throw e
        }
    }

    private suspend fun runEventLoop() {
        val error = try {
            pumpEvents()
        } catch (e: OrbitNativeException) {
            e.toOrbitException()
        } catch (e: CancellationException) {
            OrbitException(OrbitErrorCode.Closed, "client was cancelled")
        } catch (e: Exception) {
            OrbitException(OrbitErrorCode.Internal, "event stream failed: ${e.message}")
        }
        finish(error)
    }

    private suspend fun pumpEvents(): Nothing {
        while (true) {
            val batch = decodeBatch(engine.waitEvents(WAIT_TIMEOUT_MS))
            for (sequenced in batch.events) dispatch(sequenced.event)
        }
    }

    private suspend fun dispatch(event: WireEvent) {
        when (event) {
            WireEvent.ContactsChanged -> mutableEvents.emit(OrbitEvent.ContactsChanged)
            is WireEvent.NetworkChanged -> mutableEvents.emit(OrbitEvent.NetworkChanged(event.network))
            is WireEvent.CommandSucceeded -> lock.withLock { pending.remove(event.requestId) }?.complete(event.result)
            is WireEvent.CommandFailed -> lock.withLock { pending.remove(event.requestId) }
                ?.completeExceptionally(OrbitException(OrbitErrorCode.of(event.error.code), event.error.message))
            is WireEvent.MessageAdded -> mutableEvents.emit(OrbitEvent.MessageAdded(event.message))
            is WireEvent.ProfileChanged -> mutableEvents.emit(OrbitEvent.ProfileChanged(event.profile))
            WireEvent.ResyncRequired -> mutableEvents.emit(OrbitEvent.ResyncRequired)
        }
    }

    private suspend fun finish(error: OrbitException) {
        withContext(NonCancellable) {
            lock.withLock {
                terminalError = error
                // Publish the state before waking callers so they observe it.
                mutableState.value =
                    if (error.code == OrbitErrorCode.Closed) ClientState.Closed else ClientState.Failed(error)
                pending.values.forEach { it.completeExceptionally(error) }
                pending.clear()
            }
        }
    }

    private companion object {
        const val WAIT_TIMEOUT_MS = 30_000
    }
}

internal fun OrbitNativeException.toOrbitException() = OrbitException(errorCode, message ?: errorCode.wireName)
