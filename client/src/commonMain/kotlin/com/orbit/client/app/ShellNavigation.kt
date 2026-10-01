package com.orbit.client.app

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/** Screen-level navigation that platform shells (keyboard shortcuts, back button) can drive. */
class ShellNavigation {
    var settingsOpen by mutableStateOf(false)
    var contactsOpen by mutableStateOf(false)

    /** Handles a "back" action; returns false when there was nothing to close. */
    fun back(): Boolean {
        if (settingsOpen) settingsOpen = false
        else if (contactsOpen) contactsOpen = false
        else return false
        return true
    }
}
