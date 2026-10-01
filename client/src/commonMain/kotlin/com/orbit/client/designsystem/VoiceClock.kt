package com.orbit.client.designsystem

/** m:ss, rounded up. commonMain so Kotlin/Native does not call JVM String.format. */
fun formatVoiceClock(durationMs: Int): String {
    val seconds = (durationMs.coerceAtLeast(0) + 999) / 1000
    val minutes = seconds / 60
    val rest = seconds % 60
    val padded = if (rest < 10) "0$rest" else rest.toString()
    return "$minutes:$padded"
}
