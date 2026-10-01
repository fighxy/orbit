package com.orbit.client.features.host

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class HostCardTest {
    @Test
    fun readsAddressAndCodeFromTheNodeCard() {
        val text = """
            2026-10-01 INFO orbit-node is listening address=ignored
            ОК.

            node address: abcdef@192.168.1.20:7443
            registration code: WBD8PSRGZCJULYEGCT3U

            Адрес можно отправить.
        """.trimIndent()
        assertEquals(
            HostedNode("abcdef@192.168.1.20:7443", "WBD8PSRGZCJULYEGCT3U"),
            HostCard.parse(text),
        )
    }

    @Test
    fun waitsUntilBothLinesExist() {
        assertNull(HostCard.parse("node address: abcdef@192.168.1.20:7443\n"))
        assertNull(HostCard.parse("registration code: WBD8PSRGZCJULYEGCT3U\n"))
        assertNull(HostCard.parse("node address: 0.0.0.0:7443\nregistration code: SHORT\n"))
    }
}
