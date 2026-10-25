package com.orbit.client.features.settings

/** Result of the platform photo picker after it has scaled a JPEG the core will accept. */
sealed interface AvatarPick {
    data class Image(val base64: String) : AvatarPick
    data object Cancelled : AvatarPick
    data class Rejected(val reason: String) : AvatarPick
}
