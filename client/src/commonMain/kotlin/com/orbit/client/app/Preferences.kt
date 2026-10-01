package com.orbit.client.app

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

enum class ThemeMode { System, Light, Dark }

/** Which key combination sends a message; the other inserts a line break. */
enum class SendShortcut { Enter, CtrlEnter }

/** Interface settings. Not secret; kept outside the engine so they apply before unlock. */
data class UiPreferences(
    val theme: ThemeMode = ThemeMode.System,
    val sendShortcut: SendShortcut = SendShortcut.Enter,
)

/** Small key-value storage provided by the platform (registry/Preferences, SharedPreferences, NSUserDefaults). */
interface PreferencesStorage {
    fun read(key: String): String?

    fun write(key: String, value: String)
}

class InMemoryPreferencesStorage : PreferencesStorage {
    private val values = HashMap<String, String>()

    override fun read(key: String): String? = values[key]

    override fun write(key: String, value: String) {
        values[key] = value
    }
}

class PreferencesRepository(private val storage: PreferencesStorage) {
    private val mutableState = MutableStateFlow(load())
    val state: StateFlow<UiPreferences> = mutableState.asStateFlow()

    fun setTheme(theme: ThemeMode) {
        storage.write(KEY_THEME, theme.name)
        mutableState.update { it.copy(theme = theme) }
    }

    fun setSendShortcut(shortcut: SendShortcut) {
        storage.write(KEY_SEND, shortcut.name)
        mutableState.update { it.copy(sendShortcut = shortcut) }
    }

    private fun load() = UiPreferences(
        theme = storage.read(KEY_THEME).toEnum(ThemeMode.System),
        sendShortcut = storage.read(KEY_SEND).toEnum(SendShortcut.Enter),
    )

    private companion object {
        const val KEY_THEME = "ui.theme"
        const val KEY_SEND = "ui.sendShortcut"
    }
}

private inline fun <reified T : Enum<T>> String?.toEnum(default: T): T =
    enumValues<T>().firstOrNull { it.name == this } ?: default
