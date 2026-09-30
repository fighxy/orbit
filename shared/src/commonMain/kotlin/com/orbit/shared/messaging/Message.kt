package com.orbit.shared.messaging

import com.orbit.shared.identity.Identity

/**
 * A single message inside a room.
 *
 * Payload is encrypted end-to-end; the Autobase log only ever sees ciphertext.
 * compact-encoding (JS layer) handles the binary wire format.
 */
sealed class Message {
    abstract val id: String
    abstract val roomId: String
    abstract val sender: Identity
    abstract val timestamp: Long

    data class Text(
        override val id: String,
        override val roomId: String,
        override val sender: Identity,
        override val timestamp: Long,
        val text: String
    ) : Message()

    data class Media(
        override val id: String,
        override val roomId: String,
        override val sender: Identity,
        override val timestamp: Long,
        val mediaId: String,
        val mimeType: String,
        val size: Long
    ) : Message()

    data class System(
        override val id: String,
        override val roomId: String,
        override val sender: Identity,
        override val timestamp: Long,
        val event: String
    ) : Message()
}
