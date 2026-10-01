package com.orbit.sdk

import com.orbit.sdk.bridge.IDENTITY_LOCKED_TAG
import com.orbit.sdk.bridge.NativeLibrary
import com.orbit.sdk.bridge.OrbitErrorCode
import com.orbit.sdk.bridge.OrbitNativeException
import com.orbit.sdk.bridge.SUPPORTED_ABI_VERSION
import com.orbit.sdk.platform.SecretStore
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/** What is stored for the local account. */
enum class IdentityStatus {
    /** No account on this device yet. */
    Missing,

    /** The account opens without user input. */
    Ready,

    /** The account secret is sealed under a passcode. */
    PasscodeRequired,
}

/**
 * Creates, protects and opens the local account.
 *
 * The identity secret moves only between the native engine and [secretStore];
 * it is wiped from Kotlin memory right after use and never appears in models.
 * An optional passcode seals the stored secret with Argon2id, so the secure
 * store alone no longer opens the account.
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

    suspend fun identityStatus(): IdentityStatus {
        val stored = secretStore.read(identityKey) ?: return IdentityStatus.Missing
        val locked = stored.isLocked()
        stored.fill(0)
        return if (locked) IdentityStatus.PasscodeRequired else IdentityStatus.Ready
    }

    /** Generates a new account and device identity, optionally sealed under [passcode]. */
    suspend fun createIdentity(passcode: String? = null) {
        check(identityStatus() == IdentityStatus.Missing) { "an identity already exists for key '$identityKey'" }
        val secret = native { generateIdentity() }
        try {
            store(secret, passcode)
        } finally {
            secret.fill(0)
        }
    }

    /** Opens the engine. [passcode] is required when the status is [IdentityStatus.PasscodeRequired]. */
    suspend fun open(passcode: String? = null): OrbitClient {
        val secret = readPlainSecret(passcode)
        try {
            val config = buildJsonObject { put("data_dir", dataDir) }.toString().encodeToByteArray()
            val engine = native { open(config, secret) }
            return OrbitClient(engine, ioDispatcher)
        } finally {
            secret.fill(0)
        }
    }

    /** Sets or changes the passcode. [currentPasscode] is required when one is set. */
    suspend fun setPasscode(currentPasscode: String?, newPasscode: String) {
        val secret = readPlainSecret(currentPasscode)
        try {
            store(secret, newPasscode)
        } finally {
            secret.fill(0)
        }
    }

    /** Removes the passcode after verifying [currentPasscode]. */
    suspend fun removePasscode(currentPasscode: String) {
        val secret = readPlainSecret(currentPasscode)
        try {
            store(secret, passcode = null)
        } finally {
            secret.fill(0)
        }
    }

    private suspend fun readPlainSecret(passcode: String?): ByteArray {
        val stored = secretStore.read(identityKey)
            ?: throw OrbitException(OrbitErrorCode.NotFound, "no identity is stored for key '$identityKey'")
        if (!stored.isLocked()) return stored
        try {
            if (passcode == null) throw OrbitException(OrbitErrorCode.WrongPasscode, "passcode is required")
            val passcodeBytes = passcode.encodeToByteArray()
            try {
                return native { unlockIdentity(stored, passcodeBytes) }
            } finally {
                passcodeBytes.fill(0)
            }
        } finally {
            stored.fill(0)
        }
    }

    private suspend fun store(secret: ByteArray, passcode: String?) {
        if (passcode == null) {
            secretStore.write(identityKey, secret)
            return
        }
        val passcodeBytes = passcode.encodeToByteArray()
        try {
            val locked = native { lockIdentity(secret, passcodeBytes) }
            try {
                secretStore.write(identityKey, locked)
            } finally {
                locked.fill(0)
            }
        } finally {
            passcodeBytes.fill(0)
        }
    }

    /** Runs a blocking native call off the caller's thread with typed errors. */
    private suspend fun <T> native(block: NativeLibrary.() -> T): T = withContext(ioDispatcher) {
        try {
            native.block()
        } catch (e: OrbitNativeException) {
            throw e.toOrbitException()
        }
    }

    private fun ByteArray.isLocked(): Boolean = isNotEmpty() && this[0] == IDENTITY_LOCKED_TAG

    companion object {
        const val DEFAULT_IDENTITY_KEY: String = "identity/default"
    }
}
