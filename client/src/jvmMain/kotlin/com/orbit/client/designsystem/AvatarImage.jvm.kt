package com.orbit.client.designsystem

import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.toComposeImageBitmap
import java.io.ByteArrayInputStream
import javax.imageio.ImageIO

internal actual fun decodeAvatarBytes(bytes: ByteArray): ImageBitmap? {
    if (bytes.isEmpty()) return null
    val image = ImageIO.read(ByteArrayInputStream(bytes)) ?: return null
    return image.toComposeImageBitmap()
}
