package com.orbit.client.features.chat

import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.MessageId

/** What a platform recorder needs from the open account. */
interface VoiceActions {
    fun sendVoice(conversationId: ConversationId, wav: ByteArray)

    suspend fun readVoice(messageId: MessageId): ByteArray
}

/**
 * Microphone and speaker for a 16 kHz PCM note.
 * Absent on platforms that cannot record or play yet, so the buttons stay hidden.
 */
interface VoiceHost {
    val recording: Boolean
    val elapsedMs: Int
    val playingHex: String?
    val error: String?

    fun attach(actions: VoiceActions?)

    fun toggleRecord(conversationId: ConversationId)

    fun togglePlay(messageId: MessageId)

    /** Stops a take and sends it when it is long enough. */
    fun commit()

    fun dismissError()

    fun release()
}
