@file:OptIn(ExperimentalForeignApi::class)

package com.orbit.client.media

import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.usePinned
import platform.Foundation.NSData
import platform.posix.memcpy

internal fun NSData.toByteArray(): ByteArray {
    val count = length.toLong()
    if (count <= 0L || count > Int.MAX_VALUE) return ByteArray(0)
    val raw = bytes ?: return ByteArray(0)
    val out = ByteArray(count.toInt())
    out.usePinned { pinned ->
        memcpy(pinned.addressOf(0), raw, count.toULong())
    }
    return out
}

internal fun ByteArray.toNSData(): NSData {
    if (isEmpty()) return NSData()
    return usePinned { pinned ->
        NSData.dataWithBytes(pinned.addressOf(0), size.toULong())
    }
}
