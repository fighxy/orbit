package com.orbit.desktop

import kotlin.test.Test
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking

class WindowsNodeHostTest {
    @Test
    fun becomeNodeReturnsAddressAndCode() = runBlocking {
        val executable = locateOrbitNodeExecutable() ?: error("orbit-node executable was not built")
        val host = WindowsNodeHost(executable)
        try {
            val card = host.becomeNode()
            assertTrue(card.address.contains("@") && card.address.endsWith(":7443"), card.address)
            assertTrue(card.registrationCode.length >= 12, card.registrationCode)
            val again = host.becomeNode()
            assertTrue(again == card)
        } finally {
            host.stop()
        }
    }
}
