package com.orbit.sdk

import com.orbit.sdk.bridge.NativeLibrary
import com.orbit.sdk.bridge.OrbitErrorCode
import com.orbit.sdk.bridge.OrbitNativeException
import com.orbit.sdk.bridge.SUPPORTED_ABI_VERSION
import com.orbit.sdk.platform.SecretStore
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * Creates and opens the local account.
 *
 * The identity secret moves only between the native engine and [secretStore];
 * it is wiped from Kotlin memory right after use and never appears in models.
 *
 * @param dataDir absolute, application-private directory for engine storage.
 * @param identityKey secure store key of the identity secret (one per profile).
 */
class OrbitSdk(
    private val native: NativeLibrary,
    private val secretStore: SecretStore,
    private val dataDir: String,
    private val identityKey: String = DEFAULT_IDENTITY_KEY,
    private val ioDispatcher: CoroutineDispatcher = defaultIoDispatcher,
) {
    init {
        check(native.abiVersion == SUPPORTED_ABI_VERSION) {
            "native ABI ${native.abiVersion} does not match SDK ABI $SUPPORTED_ABI_VERSION"
        }
    }

    suspend fun identityExists(): Boolean {
        val secret = secretStore.read(identityKey) ?: return false
        secret.fill(0)
        return true
    }

    /** Generates a new account and device identity and stores its secret. */
    suspend fun createIdentity() {
        check(!identityExists()) { "an identity already exists for key '$identityKey'" }
        val secret = withContext(ioDispatcher) { native.generateIdentity() }
        try {
            secretStore.write(identityKey, secret)
        } finally {
            secret.fill(0)
        }
    }

    /** Opens the engine for the stored identity. */
    suspend fun open(): OrbitClient {
        val secret = secretStore.read(identityKey)
            ?: throw OrbitException(OrbitErrorCode.NotFound, "no identity is stored for key '$identityKey'")
        try {
            val config = buildJsonObject { put("data_dir", dataDir) }.toString().encodeToByteArray()
            val engine = withContext(ioDispatcher) {
                try {
                    native.open(config, secret)
                } catch (e: OrbitNativeException) {
                    throw e.toOrbitException()
                }
            }
            return OrbitClient(engine, ioDispatcher)
        } finally {
            secret.fill(0)
        }
    }

    companion object {
        const val DEFAULT_IDENTITY_KEY: String = "identity/default"
    }
}
