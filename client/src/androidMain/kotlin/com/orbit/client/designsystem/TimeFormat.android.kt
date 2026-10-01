package com.orbit.client.designsystem

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

private val clock = DateTimeFormatter.ofPattern("HH:mm")
private val russian = Locale.forLanguageTag("ru")
private val dayMonth = DateTimeFormatter.ofPattern("d MMMM", russian)
private val dayMonthYear = DateTimeFormatter.ofPattern("d MMMM yyyy", russian)

private fun zoned(epochMs: Long) = Instant.ofEpochMilli(epochMs).atZone(ZoneId.systemDefault())

actual fun formatClockTime(epochMs: Long): String = clock.format(zoned(epochMs))

actual fun localDayIndex(epochMs: Long): Long = zoned(epochMs).toLocalDate().toEpochDay()

actual fun formatCalendarDay(epochMs: Long, includeYear: Boolean): String =
    (if (includeYear) dayMonthYear else dayMonth).format(zoned(epochMs))

actual fun localYear(epochMs: Long): Int = zoned(epochMs).year

actual fun currentTimeMs(): Long = System.currentTimeMillis()
