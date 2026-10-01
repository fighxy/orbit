package com.orbit.client.designsystem

import androidx.compose.ui.graphics.ImageBitmap

/** iOS drawing of a profile picture is not in this slice. Initials stay in its place. */
internal actual fun decodeAvatarBytes(bytes: ByteArray): ImageBitmap? = null
