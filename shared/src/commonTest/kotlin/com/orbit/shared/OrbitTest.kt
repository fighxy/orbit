package com.orbit.shared

import kotlin.test.Test
import kotlin.test.assertEquals

class OrbitTest {
    @Test
    fun versionIsSet() {
        assertEquals("0.1.0", Orbit.VERSION)
    }

    @Test
    fun greetContainsVersion() {
        assertEquals(true, Orbit.greet().contains(Orbit.VERSION))
    }
}
