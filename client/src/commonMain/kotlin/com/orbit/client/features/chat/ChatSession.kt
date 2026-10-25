package com.orbit.client.features.chat

import com.orbit.client.app.ChatBackend
import com.orbit.client.designsystem.Strings
import com.orbit.sdk.OrbitEvent
import com.orbit.sdk.OrbitException
import kotlin.io.encoding.Base64
import com.orbit.sdk.model.Conversation
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.ConversationKind
import com.orbit.sdk.model.Message
import com.orbit.sdk.model.MessageId
import com.orbit.sdk.model.MessageState
import com.orbit.sdk.model.Profile
import com.orbit.sdk.model.PublicIdentity
import com.orbit.sdk.model.Contact
import com.orbit.sdk.model.InvitePreview
import com.orbit.sdk.model.NetworkStatus
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class ChatState(
    val conversationId: ConversationId? = null,
    /** Ascending by local `seq`. */
    val messages: List<Message> = emptyList(),
    val hasMore: Boolean = false,
    val loading: Boolean = false,
    val loadingOlder: Boolean = false,
    val sending: Int = 0,
    val error: String? = null,
)

/**
 * State of an opened account: identity, conversation list and the selected
 * conversation. All mutations happen on the scope's dispatcher (UI thread).
 */
class ChatSession(
    private val backend: ChatBackend,
    parentScope: CoroutineScope,
) {
    private val scope = CoroutineScope(
        parentScope.coroutineContext + SupervisorJob(parentScope.coroutineContext[Job]),
    )

    private val mutableIdentity = MutableStateFlow<PublicIdentity?>(null)
    val identity: StateFlow<PublicIdentity?> = mutableIdentity.asStateFlow()

    private val mutableNetwork = MutableStateFlow(NetworkStatus())
    val network: StateFlow<NetworkStatus> = mutableNetwork.asStateFlow()
    private var networkRevision = 0L

    suspend fun registerNode(node: String, code: String?) {
        mutableNetwork.value = backend.registerNode(node, code)
    }

    suspend fun createInvite(): String = backend.createInvite()
    suspend fun inspectInvite(text: String): InvitePreview = backend.inspectInvite(text)
    suspend fun acceptInvite(text: String): Contact {
        val contact = backend.acceptInvite(text)
        reloadSnapshot()
        select(contact.conversationId)
        return contact
    }

    fun createRoom(kind: ConversationKind, title: String, members: List<ConversationId>) {
        scope.launch {
            try {
                val created = when (kind) {
                    ConversationKind.Group -> backend.createGroup(title, members)
                    ConversationKind.Channel -> backend.createChannel(title, members)
                    else -> return@launch
                }
                reloadSnapshot()
                select(created.id)
            } catch (e: OrbitException) {
                mutableBanner.value = Strings.describe(e)
            }
        }
    }

    private val mutableProfile = MutableStateFlow<Profile?>(null)
    val profile: StateFlow<Profile?> = mutableProfile.asStateFlow()

    private val mutableConversations = MutableStateFlow<List<Conversation>>(emptyList())
    val conversations: StateFlow<List<Conversation>> = mutableConversations.asStateFlow()

    private val mutableChat = MutableStateFlow(ChatState())
    val chat: StateFlow<ChatState> = mutableChat.asStateFlow()

    private val mutableBanner = MutableStateFlow<String?>(null)

    /** Session-level problem (for example a failed resync). */
    val banner: StateFlow<String?> = mutableBanner.asStateFlow()

    fun start() {
        // Subscribe before the first snapshot so no announced message is missed.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            backend.events.collect(::onEvent)
        }
        scope.launch { reloadSnapshot() }
    }

    fun select(conversationId: ConversationId?) {
        if (conversationId == mutableChat.value.conversationId) return
        mutableChat.value = ChatState(conversationId = conversationId, loading = conversationId != null)
        if (conversationId != null) scope.launch { loadNewest(conversationId) }
    }

    fun loadOlder() {
        val state = mutableChat.value
        val conversationId = state.conversationId ?: return
        val oldest = state.messages.firstOrNull() ?: return
        if (!state.hasMore || state.loadingOlder || state.loading) return
        mutableChat.update { it.copy(loadingOlder = true) }
        scope.launch {
            try {
                val page = backend.messages(conversationId, oldest.seq, PAGE_SIZE)
                updateChat(conversationId) {
                    it.copy(messages = merge(it.messages, page.messages), hasMore = page.hasMore, loadingOlder = false)
                }
            } catch (e: OrbitException) {
                updateChat(conversationId) { it.copy(loadingOlder = false, error = Strings.describe(e)) }
            }
        }
    }

    fun send(text: String) {
        val conversationId = mutableChat.value.conversationId ?: return
        if (text.isBlank()) return
        mutableChat.update { it.copy(sending = it.sending + 1) }
        scope.launch {
            try {
                upsert(backend.sendText(conversationId, text))
                updateChat(conversationId) { it.copy(sending = it.sending - 1) }
            } catch (e: OrbitException) {
                updateChat(conversationId) { it.copy(sending = it.sending - 1, error = Strings.describe(e)) }
            }
        }
    }

    fun edit(messageId: MessageId, text: String) {
        val conversationId = mutableChat.value.conversationId ?: return
        if (text.isBlank()) return
        mutableChat.update { it.copy(sending = it.sending + 1) }
        scope.launch {
            try {
                upsert(backend.editText(conversationId, messageId, text))
                updateChat(conversationId) { it.copy(sending = (it.sending - 1).coerceAtLeast(0)) }
            } catch (e: OrbitException) {
                updateChat(conversationId) { it.copy(sending = (it.sending - 1).coerceAtLeast(0), error = Strings.describe(e)) }
            }
        }
    }

    fun delete(messageId: MessageId) {
        val conversationId = mutableChat.value.conversationId ?: return
        scope.launch {
            try {
                upsert(backend.deleteText(conversationId, messageId))
            } catch (e: OrbitException) {
                updateChat(conversationId) { it.copy(error = Strings.describe(e)) }
            }
        }
    }

    fun sendVoice(conversationId: ConversationId, wav: ByteArray) {
        val encoded = Base64.Default.encode(wav)
        mutableChat.update { if (it.conversationId == conversationId) it.copy(sending = it.sending + 1) else it }
        scope.launch {
            try {
                upsert(backend.sendVoice(conversationId, encoded))
                updateChat(conversationId) { it.copy(sending = (it.sending - 1).coerceAtLeast(0)) }
            } catch (e: OrbitException) {
                updateChat(conversationId) { it.copy(sending = (it.sending - 1).coerceAtLeast(0), error = Strings.describe(e)) }
            }
        }
    }

    suspend fun readVoice(messageId: MessageId): ByteArray = Base64.Default.decode(backend.readVoice(messageId))

    fun dismissError() = mutableChat.update { it.copy(error = null) }

    /** Saves the profile; returns a user-facing error or null on success. */
    suspend fun updateProfile(displayName: String, about: String): String? = try {
        mutableProfile.value = backend.updateProfile(displayName, about)
        null
    } catch (e: OrbitException) {
        Strings.describe(e)
    }

    /** Sets or clears the profile picture. An empty string clears it. */
    suspend fun setAvatar(imageBase64: String): String? = try {
        mutableProfile.value = backend.setAvatar(imageBase64)
        null
    } catch (e: OrbitException) {
        Strings.describe(e)
    }

    fun showBanner(message: String) {
        mutableBanner.value = message
    }

    fun dismissBanner() {
        mutableBanner.value = null
    }

    suspend fun close() {
        scope.cancel()
        backend.close()
    }

    private suspend fun onEvent(event: OrbitEvent) {
        when (event) {
            OrbitEvent.ContactsChanged -> reloadSnapshot()
            is OrbitEvent.NetworkChanged -> { networkRevision++; mutableNetwork.value = event.network }
            is OrbitEvent.MessageAdded -> upsert(event.message)
            is OrbitEvent.ProfileChanged -> mutableProfile.value = event.profile
            OrbitEvent.ResyncRequired -> {
                reloadSnapshot()
                mutableChat.value.conversationId?.let { loadNewest(it) }
            }
        }
    }

    private suspend fun reloadSnapshot() {
        try {
            val revision = networkRevision
            val snapshot = backend.snapshot()
            mutableIdentity.value = snapshot.identity
            mutableProfile.value = snapshot.profile
            val current = mutableConversations.value.associateBy { it.id }
            mutableConversations.value = snapshot.conversations.map { incoming ->
                val old = current[incoming.id]
                val last = old?.lastMessage
                val newest = incoming.lastMessage
                incoming.copy(lastMessage = when {
                    last == null -> newest
                    newest == null || last.seq > newest.seq -> last
                    last.id == newest.id -> advancedMessage(last, newest)
                    else -> newest
                }, contact = incoming.contact?.let { contact ->
                    if (old?.contact?.ready == true) contact.copy(ready = true) else contact
                })
            }
            if (revision == networkRevision) mutableNetwork.value = snapshot.network
            mutableBanner.value = null
        } catch (e: OrbitException) {
            mutableBanner.value = Strings.describe(e)
        }
    }

    private suspend fun loadNewest(conversationId: ConversationId) {
        try {
            val page = backend.messages(conversationId, null, PAGE_SIZE)
            updateChat(conversationId) {
                // Keep messages that arrived while the page was loading.
                it.copy(messages = merge(it.messages, page.messages), hasMore = page.hasMore, loading = false)
            }
        } catch (e: OrbitException) {
            updateChat(conversationId) { it.copy(loading = false, error = Strings.describe(e)) }
        }
    }

    private fun upsert(message: Message) {
        mutableConversations.update { list ->
            list.map { conversation ->
                val last = conversation.lastMessage
                if (conversation.id == message.conversationId && (last == null || last.seq <= message.seq)) {
                    conversation.copy(lastMessage = if (last?.id == message.id) advancedMessage(last, message) else message)
                } else {
                    conversation
                }
            }.sortedByDescending { it.lastMessage?.createdAtMs ?: it.createdAtMs }
        }
        updateChat(message.conversationId) { it.copy(messages = merge(it.messages, listOf(message))) }
    }

    /** Applies [change] only if [conversationId] is still the open conversation. */
    private inline fun updateChat(conversationId: ConversationId, change: (ChatState) -> ChatState) {
        mutableChat.update { if (it.conversationId == conversationId) change(it) else it }
    }

    private companion object {
        const val PAGE_SIZE = 50

        fun merge(current: List<Message>, incoming: List<Message>): List<Message> =
            current.associateBy { it.id }.toMutableMap().apply {
                incoming.forEach { message -> this[message.id] = this[message.id]?.let { advancedMessage(it, message) } ?: message }
            }.values.sortedBy { it.seq }
    }
}

/** A delayed send/snapshot result must not undo an already observed receipt. */
private fun advancedMessage(old: Message, incoming: Message): Message {
    fun rank(state: MessageState): Int = when (state) {
        MessageState.SavedLocally -> 0
        MessageState.Queued -> 1
        MessageState.Mailbox -> 2
        MessageState.Delivered, MessageState.Received -> 3
    }
    val content = when {
        incoming.revision > old.revision -> incoming
        incoming.revision < old.revision -> old
        incoming.deleted && !old.deleted -> incoming
        else -> old
    }
    val state = if (rank(old.state) >= rank(incoming.state)) old.state else incoming.state
    return content.copy(state = state)
}
