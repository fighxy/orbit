package com.orbit.shared.room

import com.orbit.shared.identity.Identity
import com.orbit.shared.messaging.Message

/**
 * A chat room = one Autobase.
 *
 * Every member is a writer; Autobase linearises writes into a shared,
 * eventually consistent view. Media lives in the room's Hyperdrive.
 */
data class Room(
    val id: String,
    val name: String,
    val members: List<Identity>,
    val createdAt: Long
)

interface RoomRepository {
    suspend fun create(name: String, creator: Identity): Room
    suspend fun join(inviteCode: String): Room
    suspend fun send(roomId: String, message: Message)
    suspend fun history(roomId: String, limit: Int = 50): List<Message>
}
