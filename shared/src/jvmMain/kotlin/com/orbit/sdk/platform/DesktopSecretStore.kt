package com.orbit.sdk.platform

import com.orbit.sdk.bridge.JniNativeLibrary
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** JNI declarations implemented in `crates/orbit-ffi/src/desktop_keyring.rs`. */
internal object DesktopKeyring {
    @JvmStatic external fun nativeRead(service: String, account: String): ByteArray?

    @JvmStatic external fun nativeWrite(service: String, account: String, secret: ByteArray)

    @JvmStatic external fun nativeDelete(service: String, account: String)
}

/**
 * Secret store backed by the OS keyring: macOS Keychain, Windows Credential
 * Manager, or the Secret Service (GNOME Keyring, KWallet) on Linux. Fails
 * with [SecureStorageException] when the keyring is unavailable; there is no
 * file fallback.
 *
 * Requires the native library, so it takes the loaded [JniNativeLibrary].
 */
class DesktopSecretStore(
    @Suppress("unused") private val library: JniNativeLibrary,
    private val service: String = DEFAULT_SERVICE,
) : SecretStore {
    override suspend fun read(key: String): ByteArray? = withContext(Dispatchers.IO) {
        DesktopKeyring.nativeRead(service, key)
    }

    override suspend fun write(key: String, value: ByteArray) = withContext(Dispatchers.IO) {
        DesktopKeyring.nativeWrite(service, key, value)
    }

    override suspend fun delete(key: String) = withContext(Dispatchers.IO) {
        DesktopKeyring.nativeDelete(service, key)
    }

    companion object {
        const val DEFAULT_SERVICE: String = "com.orbit.messenger"
    }
}
