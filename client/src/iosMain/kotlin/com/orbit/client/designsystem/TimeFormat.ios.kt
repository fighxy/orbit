package com.orbit.client.designsystem

import platform.Foundation.NSDate
import platform.Foundation.NSDateFormatter
import platform.Foundation.NSLocale
import platform.Foundation.NSTimeZone
import platform.Foundation.dateWithTimeIntervalSince1970
import platform.Foundation.localTimeZone
import platform.Foundation.timeIntervalSince1970

private val clock = NSDateFormatter().apply { dateFormat = "HH:mm" }
private val russian = NSLocale(localeIdentifier = "ru_RU")
private val dayMonth = NSDateFormatter().apply {
    locale = russian
    dateFormat = "d MMMM"
}
private val dayMonthYear = NSDateFormatter().apply {
    locale = russian
    dateFormat = "d MMMM yyyy"
}

private fun date(epochMs: Long) = NSDate.dateWithTimeIntervalSince1970(epochMs / 1000.0)

actual fun formatClockTime(epochMs: Long): String = clock.stringFromDate(date(epochMs))

actual fun localDayIndex(epochMs: Long): Long {
    val offsetSeconds = NSTimeZone.localTimeZone.secondsFromGMTForDate(date(epochMs))
    return (epochMs / 1000 + offsetSeconds).floorDiv(86_400L)
}

actual fun formatCalendarDay(epochMs: Long, includeYear: Boolean): String =
    (if (includeYear) dayMonthYear else dayMonth).stringFromDate(date(epochMs))

private val yearFormatter = NSDateFormatter().apply { dateFormat = "yyyy" }

actual fun localYear(epochMs: Long): Int = yearFormatter.stringFromDate(date(epochMs)).toInt()

actual fun currentTimeMs(): Long = (NSDate().timeIntervalSince1970 * 1000).toLong()
