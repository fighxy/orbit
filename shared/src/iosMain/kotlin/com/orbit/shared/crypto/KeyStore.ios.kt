package com.orbit.shared.crypto

import platform.Foundation.NSData
import platform.Security.*

/**
 * iOS Keychain-backed implementation.
 * Seed phrase stored with kSecAttrAccessibleWhenUnlockedThisDeviceOnly.
 */
actual class KeyStore actual constructor() {
    actual suspend fun saveSeedPhrase(phrase: List<String>) {
        TODO("Store in Keychain via Security framework")
    }

    actual suspend fun loadSeedPhrase(): List<String>? {
        TODO("Load from Keychain via Security framework")
    }

    actual suspend fun clear() {
        TODO("Delete from Keychain")
    }
}
