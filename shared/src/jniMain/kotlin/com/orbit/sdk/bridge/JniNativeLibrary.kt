package com.orbit.sdk.bridge

/**
 * JNI declarations implemented in `crates/orbit-ffi/src/jni_api.rs`.
 * Class name, method names and signatures are part of the native ABI.
 */
internal object OrbitJni {
    @JvmStatic external fun nativeAbiVersion(): Int

    @JvmStatic external fun nativeGenerateIdentity(): ByteArray

    @JvmStatic external fun nativeLockIdentity(secret: ByteArray, passcode: ByteArray): ByteArray

    @JvmStatic external fun nativeUnlockIdentity(locked: ByteArray, passcode: ByteArray): ByteArray

    @JvmStatic external fun nativeOpen(configJson: ByteArray, secret: ByteArray): Long

    @JvmStatic external fun nativeSubmit(engine: Long, commandJson: ByteArray): Long

    @JvmStatic external fun nativeWaitEvents(engine: Long, timeoutMs: Int): ByteArray

    @JvmStatic external fun nativeCancelWait(engine: Long)

    @JvmStatic external fun nativeClose(engine: Long)
}

/** [NativeLibrary] over JNI. Obtain it after the shared library is loaded. */
class JniNativeLibrary internal constructor() : NativeLibrary {
    override val abiVersion: Int get() = OrbitJni.nativeAbiVersion()

    override fun generateIdentity(): ByteArray = OrbitJni.nativeGenerateIdentity()

    override fun lockIdentity(secret: ByteArray, passcode: ByteArray): ByteArray =
        OrbitJni.nativeLockIdentity(secret, passcode)

    override fun unlockIdentity(locked: ByteArray, passcode: ByteArray): ByteArray =
        OrbitJni.nativeUnlockIdentity(locked, passcode)

    override fun open(configJson: ByteArray, secret: ByteArray): NativeEngine =
        JniNativeEngine(OrbitJni.nativeOpen(configJson, secret))

    companion object {
        private val lock = Any()
        private var loaded: JniNativeLibrary? = null

        /**
         * Loads `orbit_ffi` once: from [absolutePath] when given (desktop
         * development and packaged apps), otherwise from the library path.
         */
        fun load(absolutePath: String? = null): JniNativeLibrary = synchronized(lock) {
            loaded ?: run {
                if (absolutePath != null) System.load(absolutePath) else System.loadLibrary("orbit_ffi")
                JniNativeLibrary().also { loaded = it }
            }
        }
    }
}

private class JniNativeEngine(private val handle: Long) : NativeEngine {
    override fun submit(commandJson: ByteArray): Long = OrbitJni.nativeSubmit(handle, commandJson)

    override fun waitEvents(timeoutMs: Int): ByteArray = OrbitJni.nativeWaitEvents(handle, timeoutMs)

    override fun cancelWait() = OrbitJni.nativeCancelWait(handle)

    override fun close() = OrbitJni.nativeClose(handle)
}
