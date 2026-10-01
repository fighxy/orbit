package com.orbit.client.features.chat

import com.orbit.sdk.model.AccountId
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.DeviceId
import com.orbit.sdk.model.Message
import com.orbit.sdk.model.MessageBody
import com.orbit.sdk.model.MessageId
import com.orbit.sdk.model.MessageState
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val DAY = 86_400_000L
private const val MINUTE = 60_000L

private fun message(seq: Long, at: Long, device: String = "02") = Message(
    id = MessageId(seq.toString().padStart(32, '0')),
    conversationId = ConversationId("cd".repeat(16)),
    seq = seq,
    authorAccount = AccountId("01".repeat(32)),
    authorDevice = DeviceId(device.repeat(32)),
    createdAtMs = at,
    body = MessageBody.Text("m$seq"),
    state = MessageState.SavedLocally,
)

private fun timeline(messages: List<Message>) =
    buildTimeline(messages, dayIndex = { it / DAY }, dayLabel = { "day ${it / DAY}" })

class TimelineTest {
    @Test
    fun emptyHistoryHasNoRows() {
        assertTrue(timeline(emptyList()).isEmpty())
    }

    @Test
    fun separatorPrecedesEachNewDay() {
        val rows = timeline(listOf(message(1, 10), message(2, DAY + 10), message(3, DAY + 20)))
        val labels = rows.map {
            when (it) {
                is TimelineItem.Day -> it.label
                is TimelineItem.Entry -> "m${it.message.seq}"
            }
        }
        assertEquals(listOf("day 0", "m1", "day 1", "m2", "m3"), labels)
    }

    @Test
    fun groupsByAuthorAndTimeWindow() {
        val rows = timeline(
            listOf(
                message(1, 0),
                message(2, 2 * MINUTE),
                message(3, 2 * MINUTE + GROUP_WINDOW_MS + 1), // gap too large
                message(4, 2 * MINUTE + GROUP_WINDOW_MS + 2, device = "03"), // other device
            ),
        ).filterIsInstance<TimelineItem.Entry>()
        val flags = rows.map { it.firstInGroup to it.lastInGroup }
        assertEquals(listOf(true to false, false to true, true to true, true to true), flags)
    }

    @Test
    fun keysAreUnique() {
        val rows = timeline(listOf(message(1, 0), message(2, DAY), message(3, 2 * DAY)))
        assertEquals(rows.size, rows.map { it.key }.toSet().size)
    }
}
