package com.orbit.client.app

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/** Screen-level navigation that platform shells (keyboard shortcuts, back button) can drive. */
class ShellNavigation {
    var settingsOpen by mutableStateOf(false)
    var contactsOpen by mutableStateOf(false)
    var hostOpen by mutableStateOf(false)

    /** A narrow layout has a conversation covering the list. */
    var chatOpen by mutableStateOf(false)
    var leaveChat: (() -> Unit)? = null

    val handlesBack: Boolean
        get() = hostOpen || settingsOpen || contactsOpen || chatOpen

    fun openHost() {
        settingsOpen = false
        contactsOpen = false
        hostOpen = true
    }

    /** Handles a "back" action; returns false when there was nothing to close. */
    fun back(): Boolean {
        if (hostOpen) hostOpen = false
        else if (settingsOpen) settingsOpen = false
        else if (contactsOpen) contactsOpen = false
        else if (chatOpen) leaveChat?.invoke()
        else return false
        return true
    }
}
