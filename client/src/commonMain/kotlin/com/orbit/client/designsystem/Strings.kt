package com.orbit.client.designsystem

import com.orbit.sdk.OrbitException
import com.orbit.sdk.bridge.OrbitErrorCode

/** UI text. Kept in one place so localization can replace it later. */
object Strings {
    fun voiceClock(durationMs: Int): String = formatVoiceClock(durationMs)
}
