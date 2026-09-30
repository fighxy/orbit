package com.orbit.shared.crypto

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore as AndroidKeyStore

/**
 * Android Keystore-backed implementation.
 * Seed phrase encrypted with AES-GCM, key in TEE/StrongBox.
 */
actual class KeyStore actual constructor() {
    private val androidKeyStore = AndroidKeyStore.getInstance("AndroidKeyStore")

    init {
        androidKeyStore.load(null)
    }

    actual suspend fun saveSeedPhrase(phrase: List<String>) {
        TODO("Encrypt with AES-GCM key in AndroidKeyStore")
    }

    actual suspend fun loadSeedPhrase(): List<String>? {
        TODO("Decrypt from AndroidKeyStore")
    }

    actual suspend fun clear() {
        TODO("Remove key from AndroidKeyStore")
    }
}
