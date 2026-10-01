package com.orbit.client.features.chat

import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.orbit.client.app.PreferencesRepository
import com.orbit.client.app.ShellNavigation
import com.orbit.client.features.chatlist.ChatListPane
import com.orbit.client.features.settings.SecurityActions
import com.orbit.client.features.settings.SettingsScreen

/** Two panes on wide windows; list or chat on narrow screens. */
@Composable
fun MessengerScreen(
    session: ChatSession,
    security: SecurityActions,
    preferences: PreferencesRepository,
    navigation: ShellNavigation,
) {
    if (navigation.settingsOpen) {
        SettingsScreen(session, security, preferences, onBack = { navigation.settingsOpen = false })
        return
    }
    val prefs by preferences.state.collectAsState()
    val identity by session.identity.collectAsState()
    val profile by session.profile.collectAsState()
    val conversations by session.conversations.collectAsState()
    val chat by session.chat.collectAsState()
    val banner by session.banner.collectAsState()

    BoxWithConstraints(Modifier.fillMaxSize()) {
        val wide = maxWidth >= 720.dp
        if (wide) {
            // Open the first conversation so the wide layout is never empty.
            LaunchedEffect(conversations.firstOrNull()?.id, chat.conversationId) {
                if (chat.conversationId == null) conversations.firstOrNull()?.let { session.select(it.id) }
            }
            Row(Modifier.fillMaxSize()) {
                ChatListPane(
                    identity = identity,
                    conversations = conversations,
                    selected = chat.conversationId,
                    profileName = profile?.displayName,
                    banner = banner,
                    onSelect = session::select,
                    onOpenSettings = { navigation.settingsOpen = true },
                    onDismissBanner = session::dismissBanner,
                    modifier = Modifier.width(320.dp).fillMaxHeight(),
                )
                VerticalDivider()
                ChatPane(
                    state = chat,
                    conversation = conversations.firstOrNull { it.id == chat.conversationId },
                    sendShortcut = prefs.sendShortcut,
                    onSend = session::send,
                    onLoadOlder = session::loadOlder,
                    onDismissError = session::dismissError,
                    onBack = null,
                    modifier = Modifier.fillMaxSize(),
                )
            }
        } else if (chat.conversationId == null) {
            ChatListPane(
                identity = identity,
                conversations = conversations,
                selected = null,
                profileName = profile?.displayName,
                banner = banner,
                onSelect = session::select,
                onOpenSettings = { navigation.settingsOpen = true },
                onDismissBanner = session::dismissBanner,
                modifier = Modifier.fillMaxSize(),
            )
        } else {
            ChatPane(
                state = chat,
                conversation = conversations.firstOrNull { it.id == chat.conversationId },
                sendShortcut = prefs.sendShortcut,
                onSend = session::send,
                onLoadOlder = session::loadOlder,
                onDismissError = session::dismissError,
                onBack = { session.select(null) },
                modifier = Modifier.fillMaxSize(),
            )
        }
    }
}
