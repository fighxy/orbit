package com.orbit.shared.db

import java.io.File
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import java.nio.file.StandardOpenOption

/**
 * Desktop implementation backed by a local SQLite file.
 *
 * One database file per platform data dir:
 * - Windows: %APPDATA%\Orbit\orbit.db
 * - macOS:   ~/Library/Application Support/Orbit/orbit.db
 * - Linux:   ~/.local/share/Orbit/orbit.db
 *
 * Uses raw file I/O as a stub; swap for a real SQLite driver
 * (e.g. SQLite JDBC) when the schema stabilises.
 */
actual class LocalDatabase actual constructor() {
    private val dbFile: File by lazy {
        val dataDir = when {
            System.getProperty("os.name").lowercase().contains("win") ->
                Paths.get(System.getenv("APPDATA") ?: "", "Orbit")
            System.getProperty("os.name").lowercase().contains("mac") ->
                Paths.get(System.getProperty("user.home"), "Library", "Application Support", "Orbit")
            else ->
                Paths.get(System.getProperty("user.home"), ".local", "share", "Orbit")
        }
        Files.createDirectories(dataDir)
        dataDir.resolve("orbit.db").toFile()
    }

    actual suspend fun saveMessage(roomId: String, payload: ByteArray) {
        appendLine("${System.currentTimeMillis()}|msg|$roomId|${payload.size}")
    }

    actual suspend fun loadMessages(roomId: String, limit: Int): List<ByteArray> {
        if (!dbFile.exists()) return emptyList()
        return dbFile.readLines()
            .filter { it.contains("|msg|$roomId|") }
            .takeLast(limit)
            .map { it.toByteArray() }
    }

    actual suspend fun saveRoom(roomId: String, payload: ByteArray) {
        appendLine("${System.currentTimeMillis()}|room|$roomId|${payload.size}")
    }

    actual suspend fun loadRooms(): List<ByteArray> {
        if (!dbFile.exists()) return emptyList()
        return dbFile.readLines()
            .filter { it.contains("|room|") }
            .map { it.toByteArray() }
    }

    private fun appendLine(line: String) {
        Files.writeString(
            dbFile.toPath(),
            line + System.lineSeparator(),
            StandardOpenOption.CREATE,
            StandardOpenOption.APPEND
        )
    }
}
