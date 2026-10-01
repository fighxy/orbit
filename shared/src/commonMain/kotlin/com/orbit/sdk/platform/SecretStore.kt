package com.orbit.sdk.platform

import kotlin.jvm.JvmOverloads

/**
 * Platform secure storage for small secrets (Keychain, Android Keystore,
 * desktop keyring). Implementations never fall back to plaintext storage:
 * when the secure store is unavailable they throw [SecureStorageException].
 */
interface SecretStore {
    /** Returns the stored secret or `null` when none exists. */
    suspend fun read(key: String): ByteArray?

    /** Creates or replaces the secret. */
    suspend fun write(key: String, value: ByteArray)

    /** Removes the secret; absent keys are ignored. */
    suspend fun delete(key: String)
}

/** The platform secure store cannot be used. Nothing was stored in its place. */
class SecureStorageException @JvmOverloads constructor(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)
