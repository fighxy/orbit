package com.orbit.shared.db

import platform.Foundation.NSData

/**
 * iOS implementation backed by SwiftData.
 *
 * The Swift side owns the ModelContainer; this expect/actual bridge
 * forwards calls through a Kotlin-exported protocol.
 */
actual class LocalDatabase actual constructor() {
    actual suspend fun saveMessage(roomId: String, payload: ByteArray) {
        TODO("Bridge to SwiftData ModelContainer")
    }

    actual suspend fun loadMessages(roomId: String, limit: Int): List<ByteArray> {
        TODO("Bridge to SwiftData ModelContainer")
    }

    actual suspend fun saveRoom(roomId: String, payload: ByteArray) {
        TODO("Bridge to SwiftData ModelContainer")
    }

    actual suspend fun loadRooms(): List<ByteArray> {
        TODO("Bridge to SwiftData ModelContainer")
    }
}
