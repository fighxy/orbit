package com.orbit.client.designsystem

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.ImageBitmap
import kotlin.io.encoding.Base64

internal expect fun decodeAvatarBytes(bytes: ByteArray): ImageBitmap?

@Composable
internal fun rememberAvatarImage(base64: String?): ImageBitmap? {
    return remember(base64) {
        if (base64.isNullOrBlank()) {
            null
        } else {
            runCatching { decodeAvatarBytes(Base64.Default.decode(base64)) }.getOrNull()
        }
    }
}
