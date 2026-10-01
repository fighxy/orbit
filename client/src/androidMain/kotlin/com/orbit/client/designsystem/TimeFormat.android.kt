package com.orbit.client.designsystem

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

private val clock = DateTimeFormatter.ofPattern("HH:mm")

actual fun formatClockTime(epochMs: Long): String =
    clock.format(Instant.ofEpochMilli(epochMs).atZone(ZoneId.systemDefault()))
