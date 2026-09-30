package com.orbit.shared.crypto

/**
 * Secure key storage.
 *
 * - Android: Android Keystore
 * - iOS: Keychain
 *
 * Seed phrase and device keys never leave the secure enclave.
 */
expect class KeyStore() {
    suspend fun saveSeedPhrase(phrase: List<String>)
    suspend fun loadSeedPhrase(): List<String>?
    suspend fun clear()
}
