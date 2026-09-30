package com.orbit.shared.db

import androidx.room.Database
import androidx.room.RoomDatabase
import androidx.room.Entity
import androidx.room.PrimaryKey
import androidx.room.Dao
import androidx.room.Insert
import androidx.room.Query

@Entity(tableName = "messages")
data class MessageEntity(
    @PrimaryKey val id: String,
    val roomId: String,
    val payload: ByteArray,
    val timestamp: Long
)

@Dao
interface MessageDao {
    @Insert
    suspend fun insert(message: MessageEntity)

    @Query("SELECT payload FROM messages WHERE roomId = :roomId ORDER BY timestamp DESC LIMIT :limit")
    suspend fun load(roomId: String, limit: Int): List<MessageEntity>
}

@Database(entities = [MessageEntity::class], version = 1)
abstract class OrbitRoomDatabase : RoomDatabase() {
    abstract fun messageDao(): MessageDao
}

actual class LocalDatabase actual constructor() {
    // Wired to OrbitRoomDatabase via DI in the Android app module.
    // Stub kept here so the expect/actual contract compiles.
    actual suspend fun saveMessage(roomId: String, payload: ByteArray) {
        TODO("Inject OrbitRoomDatabase via DI")
    }

    actual suspend fun loadMessages(roomId: String, limit: Int): List<ByteArray> {
        TODO("Inject OrbitRoomDatabase via DI")
    }

    actual suspend fun saveRoom(roomId: String, payload: ByteArray) {
        TODO("Inject OrbitRoomDatabase via DI")
    }

    actual suspend fun loadRooms(): List<ByteArray> {
        TODO("Inject OrbitRoomDatabase via DI")
    }
}
