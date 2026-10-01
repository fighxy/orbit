package com.orbit.sdk.bridge

/**
 * Entry points of the native library. One implementation per platform:
 * JNI on Android and the JVM desktop, C interop on iOS.
 *
 * All byte arrays are UTF-8 JSON except identity secrets. Callers wipe secret
 * arrays after use.
 */
interface NativeLibrary {
    val abiVersion: Int

    /** Creates a new identity secret for the platform secure store. */
    fun generateIdentity(): ByteArray

    /** Seals [secret] under a UTF-8 [passcode] (Argon2id; takes about a second). */
    fun lockIdentity(secret: ByteArray, passcode: ByteArray): ByteArray

    /** Opens a locked secret; throws [OrbitErrorCode.WrongPasscode] on mismatch. */
    fun unlockIdentity(locked: ByteArray, passcode: ByteArray): ByteArray

    /** Opens the account described by [secret]. Blocks while storage opens. */
    fun open(configJson: ByteArray, secret: ByteArray): NativeEngine
}

/** One opened engine. Thread-safe; [waitEvents] blocks and must not run on a UI thread. */
interface NativeEngine {
    /** Queues a JSON command and returns its request ID. */
    fun submit(commandJson: ByteArray): Long

    /**
     * Blocks up to [timeoutMs] and returns `{"events":[...]}`. Throws
     * [OrbitNativeException] with [OrbitErrorCode.Closed] after [close].
     */
    fun waitEvents(timeoutMs: Int): ByteArray

    /** Makes a blocked [waitEvents] return an empty batch. */
    fun cancelWait()

    /** Stops the engine and returns after its storage lock is released. Idempotent. */
    fun close()
}

/** Error codes shared with the C ABI (`ORBIT_ERR_*`). */
enum class OrbitErrorCode(val value: Int) {
    InvalidArgument(1),
    InvalidIdentity(2),
    StorageLocked(3),
    IdentityMismatch(4),
    StorageKeyMismatch(5),
    Corrupted(6),
    UnsupportedStorageVersion(7),
    NotFound(8),
    Storage(9),
    Io(10),
    Random(11),
    Closed(12),
    Busy(13),
    Internal(14),
    WrongPasscode(15),
    NetworkNotConfigured(16),
    InvalidInvite(17),
    Network(18),
    Unknown(-1),
    ;

    companion object {
        fun of(value: Int): OrbitErrorCode = entries.firstOrNull { it.value == value } ?: Unknown

        fun of(name: String): OrbitErrorCode =
            entries.firstOrNull { it.wireName == name } ?: Unknown
    }

    /** snake_case name used in JSON error objects. */
    val wireName: String
        get() = name.replace(Regex("([a-z])([A-Z])"), "$1_$2").lowercase()
}

/**
 * Failure reported by the native engine. Thrown by JNI with the
 * `(int, String)` constructor, so its name and signature are part of the ABI.
 */
class OrbitNativeException(val code: Int, message: String) : RuntimeException(message) {
    val errorCode: OrbitErrorCode get() = OrbitErrorCode.of(code)
}

/** Native contract version this SDK was written against. */
const val SUPPORTED_ABI_VERSION: Int = 3

/** First byte of a passcode-locked identity secret (`ORBIT_IDENTITY_LOCKED_TAG`). */
const val IDENTITY_LOCKED_TAG: Byte = 0x10
