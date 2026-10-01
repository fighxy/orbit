package com.orbit.client.app

import kotlin.test.Test
import kotlin.test.assertEquals

class PreferencesRepositoryTest {
    @Test
    fun defaultsThenPersistedValuesAreLoaded() {
        val storage = InMemoryPreferencesStorage()
        val first = PreferencesRepository(storage)
        assertEquals(UiPreferences(), first.state.value)

        first.setTheme(ThemeMode.Dark)
        first.setSendShortcut(SendShortcut.CtrlEnter)
        assertEquals(ThemeMode.Dark, first.state.value.theme)

        val reloaded = PreferencesRepository(storage)
        assertEquals(UiPreferences(ThemeMode.Dark, SendShortcut.CtrlEnter), reloaded.state.value)
    }

    @Test
    fun unknownStoredValuesFallBackToDefaults() {
        val storage = InMemoryPreferencesStorage()
        storage.write("ui.theme", "Neon")
        assertEquals(ThemeMode.System, PreferencesRepository(storage).state.value.theme)
    }
}
