@file:OptIn(ExperimentalForeignApi::class)

package com.orbit.sdk.bridge

import com.orbit.sdk.ffi.ORBIT_OK
import com.orbit.sdk.ffi.OrbitBuffer
import com.orbit.sdk.ffi.orbit_abi_version
import com.orbit.sdk.ffi.orbit_buffer_free
import com.orbit.sdk.ffi.orbit_engine_cancel_wait
import com.orbit.sdk.ffi.orbit_engine_close
import com.orbit.sdk.ffi.orbit_engine_open
import com.orbit.sdk.ffi.orbit_engine_submit
import com.orbit.sdk.ffi.orbit_engine_wait_events
import com.orbit.sdk.ffi.orbit_identity_generate
import com.orbit.sdk.ffi.orbit_last_error_message
import kotlinx.cinterop.ByteVar
import kotlinx.cinterop.CPointer
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.UByteVar
import kotlinx.cinterop.ULongVar
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.alloc
import kotlinx.cinterop.convert
import kotlinx.cinterop.memScoped
import kotlinx.cinterop.ptr
import kotlinx.cinterop.readBytes
import kotlinx.cinterop.readValue
import kotlinx.cinterop.reinterpret
import kotlinx.cinterop.usePinned
import kotlinx.cinterop.value

/** [NativeLibrary] over the C ABI, linked statically into the iOS framework. */
object IosNativeLibrary : NativeLibrary {
    override val abiVersion: Int get() = orbit_abi_version().toInt()

    override fun generateIdentity(): ByteArray = memScoped {
        val buffer = alloc<OrbitBuffer>()
        checkStatus(orbit_identity_generate(buffer.ptr))
        buffer.takeBytes()
    }

    override fun open(configJson: ByteArray, secret: ByteArray): NativeEngine = memScoped {
        val handle = alloc<ULongVar>()
        configJson.withPointer { config, configLen ->
            secret.withPointer { secretPtr, secretLen ->
                checkStatus(orbit_engine_open(config, configLen, secretPtr, secretLen, handle.ptr))
            }
        }
        IosNativeEngine(handle.value)
    }
}

private class IosNativeEngine(private val handle: ULong) : NativeEngine {
    override fun submit(commandJson: ByteArray): Long = memScoped {
        val requestId = alloc<ULongVar>()
        commandJson.withPointer { command, length ->
            checkStatus(orbit_engine_submit(handle, command, length, requestId.ptr))
        }
        requestId.value.toLong()
    }

    override fun waitEvents(timeoutMs: Int): ByteArray = memScoped {
        require(timeoutMs >= 0) { "timeout must not be negative" }
        val buffer = alloc<OrbitBuffer>()
        checkStatus(orbit_engine_wait_events(handle, timeoutMs.toUInt(), buffer.ptr))
        buffer.takeBytes()
    }

    override fun cancelWait() = checkStatus(orbit_engine_cancel_wait(handle))

    override fun close() = checkStatus(orbit_engine_close(handle))
}

private fun checkStatus(status: Int) {
    if (status != ORBIT_OK) throw OrbitNativeException(status, lastErrorMessage())
}

private fun lastErrorMessage(): String = memScoped {
    val buffer = alloc<OrbitBuffer>()
    if (orbit_last_error_message(buffer.ptr) != ORBIT_OK) return@memScoped "unknown native error"
    buffer.takeBytes().decodeToString()
}

/** Copies the buffer into Kotlin memory and frees (and wipes) the native copy. */
private fun OrbitBuffer.takeBytes(): ByteArray {
    val bytes = data?.reinterpret<ByteVar>()?.readBytes(len.toInt()) ?: ByteArray(0)
    orbit_buffer_free(readValue())
    return bytes
}

private inline fun <R> ByteArray.withPointer(block: (CPointer<UByteVar>?, ULong) -> R): R =
    if (isEmpty()) {
        block(null, 0u)
    } else {
        usePinned { pinned -> block(pinned.addressOf(0).reinterpret(), size.convert()) }
    }

