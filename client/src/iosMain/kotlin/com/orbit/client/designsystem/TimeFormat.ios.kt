package com.orbit.client.designsystem

import platform.Foundation.NSDate
import platform.Foundation.NSDateFormatter
import platform.Foundation.dateWithTimeIntervalSince1970

private val clock = NSDateFormatter().apply { dateFormat = "HH:mm" }

actual fun formatClockTime(epochMs: Long): String =
    clock.stringFromDate(NSDate.dateWithTimeIntervalSince1970(epochMs / 1000.0))
