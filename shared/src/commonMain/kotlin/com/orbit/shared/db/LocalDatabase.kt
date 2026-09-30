package com.orbit.shared.db

/**
 * Platform-local database for offline-first storage.
 *
 * - Android: Room
 * - iOS: SwiftData (bridged) or SQLite via expect/actual
 *
 * Source of truth when no peers are online.
 */
expect class LocalDatabase() {
    suspend fun saveMessage(roomId: String, payload: ByteArray)
    suspend fun loadMessages(roomId: String, limit: Int): List<ByteArray>
    suspend fun saveRoom(roomId: String, payload: ByteArray)
    suspend fun loadRooms(): List<ByteArray>
}
