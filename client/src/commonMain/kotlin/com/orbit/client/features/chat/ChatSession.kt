package com.orbit.client.features.chat

import com.orbit.client.app.ChatBackend
import com.orbit.client.designsystem.Strings
import com.orbit.sdk.OrbitEvent
import com.orbit.sdk.OrbitException
import com.orbit.sdk.model.Conversation
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.Message
import com.orbit.sdk.model.Profile
import com.orbit.sdk.model.PublicIdentity
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

    fun dismissError() = mutableChat.update { it.copy(error = null) }

    /** Saves the profile; returns a user-facing error or null on success. */
    suspend fun updateProfile(displayName: String, about: String): String? = try {
        mutableProfile.value = backend.updateProfile(displayName, about)
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
            val snapshot = backend.snapshot()
            mutableIdentity.value = snapshot.identity
            mutableProfile.value = snapshot.profile
            mutableConversations.value = snapshot.conversations
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
                    conversation.copy(lastMessage = message)
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
            (current.associateBy { it.id } + incoming.associateBy { it.id }).values.sortedBy { it.seq }
    }
}
