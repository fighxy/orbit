package com.orbit.shared.crypto

import java.io.File
import java.nio.file.Files
import java.nio.file.Paths
import java.nio.file.StandardOpenOption
import java.util.Base64

/**
 * Desktop key storage.
 *
 * Seed phrase encrypted with AES-GCM using a key derived from the OS
 * keyring where available (Windows Credential Manager, macOS Keychain,
 * Linux libsecret). Falls back to an encrypted file in the data dir.
 *
 * This stub stores the phrase Base64-encoded in the data dir — replace
 * with a real keyring integration before any release build.
 */
actual class KeyStore actual constructor() {
    private val storeFile: File by lazy {
        val dataDir = when {
            System.getProperty("os.name").lowercase().contains("win") ->
                Paths.get(System.getenv("APPDATA") ?: "", "Orbit")
            System.getProperty("os.name").lowercase().contains("mac") ->
                Paths.get(System.getProperty("user.home"), "Library", "Application Support", "Orbit")
            else ->
                Paths.get(System.getProperty("user.home"), ".local", "share", "Orbit")
        }
        Files.createDirectories(dataDir)
        dataDir.resolve("identity.dat").toFile()
    }

    actual suspend fun saveSeedPhrase(phrase: List<String>) {
        val encoded = Base64.getEncoder().encodeToString(phrase.joinToString(" ").toByteArray())
        Files.writeString(storeFile.toPath(), encoded, StandardOpenOption.CREATE, StandardOpenOption.TRUNCATE_EXISTING)
    }

    actual suspend fun loadSeedPhrase(): List<String>? {
        if (!storeFile.exists()) return null
        val encoded = storeFile.readText().trim()
        if (encoded.isEmpty()) return null
        return String(Base64.getDecoder().decode(encoded)).split(" ")
    }

    actual suspend fun clear() {
        if (storeFile.exists()) storeFile.delete()
    }
}
