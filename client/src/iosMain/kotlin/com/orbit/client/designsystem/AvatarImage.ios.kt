package com.orbit.client.designsystem

import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.decodeToImageBitmap

/** Skia decode already linked by Compose. Callers still have to run this on a device. */
internal actual fun decodeAvatarBytes(bytes: ByteArray): ImageBitmap? {
    if (!isJpegOrPng(bytes)) return null
    val image = runCatching { bytes.decodeToImageBitmap() }.getOrNull() ?: return null
    if (image.width <= 0 || image.height <= 0) return null
    return image
}

private fun isJpegOrPng(bytes: ByteArray): Boolean {
    if (bytes.isEmpty() || bytes.size > 32 * 1024) return false
    val jpeg = bytes.size >= 3 &&
        bytes[0] == 0xFF.toByte() &&
        bytes[1] == 0xD8.toByte() &&
        bytes[2] == 0xFF.toByte()
    if (jpeg) return true
    if (bytes.size < 8) return false
    return bytes[0] == 0x89.toByte() &&
        bytes[1] == 0x50.toByte() &&
        bytes[2] == 0x4E.toByte() &&
        bytes[3] == 0x47.toByte() &&
        bytes[4] == 0x0D.toByte() &&
        bytes[5] == 0x0A.toByte() &&
        bytes[6] == 0x1A.toByte() &&
        bytes[7] == 0x0A.toByte()
}
