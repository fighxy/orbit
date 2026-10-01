package com.orbit.client.designsystem

/** Local wall-clock time `HH:mm` of an epoch-millisecond timestamp. */
expect fun formatClockTime(epochMs: Long): String

/** Days since 1970-01-01 in the local time zone; equal values mean the same calendar day. */
expect fun localDayIndex(epochMs: Long): Long

/** Russian month-day label: `12 сентября`, or `12 сентября 2025` outside the current year. */
expect fun formatCalendarDay(epochMs: Long, includeYear: Boolean): String

/** Calendar year in the local time zone. */
expect fun localYear(epochMs: Long): Int

expect fun currentTimeMs(): Long

/** Separator label: «Сегодня», «Вчера» or a calendar day. */
fun formatDayLabel(epochMs: Long, nowMs: Long = currentTimeMs()): String {
    val day = localDayIndex(epochMs)
    val today = localDayIndex(nowMs)
    return when (day) {
        today -> Strings.today
        today - 1 -> Strings.yesterday
        else -> formatCalendarDay(epochMs, includeYear = localYear(epochMs) != localYear(nowMs))
    }
}
