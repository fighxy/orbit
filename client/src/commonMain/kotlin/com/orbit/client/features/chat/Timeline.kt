package com.orbit.client.features.chat

import com.orbit.sdk.model.Message

/** Row of the chat history: a day separator or a message with its group position. */
sealed interface TimelineItem {
    val key: String

    data class Day(val dayIndex: Long, val label: String) : TimelineItem {
        override val key: String get() = "day-$dayIndex"
    }

    data class Entry(
        val message: Message,
        /** First message of a run by the same author within [GROUP_WINDOW_MS]. */
        val firstInGroup: Boolean,
        /** Last message of the run; it carries the bubble tail and the time. */
        val lastInGroup: Boolean,
    ) : TimelineItem {
        override val key: String get() = message.id.hex
    }
}

/** Consecutive messages closer than this belong to one visual group. */
const val GROUP_WINDOW_MS: Long = 5 * 60 * 1000

/**
 * Builds rows for [messages] in ascending order: a separator before each new
 * calendar day and group flags for consecutive messages of one author.
 */
fun buildTimeline(
    messages: List<Message>,
    dayIndex: (Long) -> Long,
    dayLabel: (Long) -> String,
): List<TimelineItem> {
    val rows = ArrayList<TimelineItem>(messages.size + 4)
    for ((index, message) in messages.withIndex()) {
        val previous = messages.getOrNull(index - 1)
        val next = messages.getOrNull(index + 1)
        val day = dayIndex(message.createdAtMs)
        if (previous == null || dayIndex(previous.createdAtMs) != day) {
            rows += TimelineItem.Day(day, dayLabel(message.createdAtMs))
        }
        rows += TimelineItem.Entry(
            message = message,
            firstInGroup = previous == null || !sameGroup(previous, message, dayIndex),
            lastInGroup = next == null || !sameGroup(message, next, dayIndex),
        )
    }
    return rows
}

private fun sameGroup(earlier: Message, later: Message, dayIndex: (Long) -> Long): Boolean =
    earlier.authorDevice == later.authorDevice &&
        dayIndex(earlier.createdAtMs) == dayIndex(later.createdAtMs) &&
        later.createdAtMs - earlier.createdAtMs in 0..GROUP_WINDOW_MS
