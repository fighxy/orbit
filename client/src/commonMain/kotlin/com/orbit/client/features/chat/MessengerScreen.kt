package com.orbit.client.features.chat

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.orbit.client.app.PreferencesRepository
import com.orbit.client.app.ShellNavigation
import com.orbit.client.features.chatlist.ChatListPane
import com.orbit.client.features.contacts.ContactsScreen
import com.orbit.client.features.host.HostNodeScreen
import com.orbit.client.features.host.NodeHost
import com.orbit.client.features.settings.AvatarPick
import com.orbit.client.features.settings.SecurityActions
import com.orbit.client.features.settings.SettingsScreen
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.MessageId

/** Two panes on wide windows; list or chat on narrow screens. */
@Composable
fun MessengerScreen(
    session: ChatSession,
    security: SecurityActions,
    preferences: PreferencesRepository,
    navigation: ShellNavigation,
    prepareLocalNetwork: (suspend () -> Boolean)? = null,
    nodeHost: NodeHost? = null,
    voice: VoiceHost? = null,
    pickAvatar: (suspend () -> AvatarPick)? = null,
) {
    DisposableEffect(session, voice) {
        voice?.attach(object : VoiceActions {
            override fun sendVoice(conversationId: ConversationId, wav: ByteArray) {
                session.sendVoice(conversationId, wav)
            }

            override suspend fun readVoice(messageId: MessageId): ByteArray = session.readVoice(messageId)
        })
        onDispose {
            voice?.attach(null)
            navigation.chatOpen = false
            navigation.leaveChat = null
        }
    }
    val prefs by preferences.state.collectAsState()
    val identity by session.identity.collectAsState()
    val profile by session.profile.collectAsState()
    val conversations by session.conversations.collectAsState()
    val chat by session.chat.collectAsState()
    val banner by session.banner.collectAsState()

    BoxWithConstraints(Modifier.fillMaxSize()) {
        val wide = maxWidth >= 720.dp
        val overlay = navigation.hostOpen || navigation.contactsOpen || navigation.settingsOpen
        val openOnPhone = !wide && chat.conversationId != null && !overlay
        SideEffect {
            navigation.chatOpen = openOnPhone
            navigation.leaveChat = if (openOnPhone) {
                { session.select(null) }
            } else {
                null
            }
        }
        if (!wide) {
            when {
                nodeHost != null && navigation.hostOpen -> HostNodeScreen(nodeHost, onBack = { navigation.hostOpen = false })
                navigation.contactsOpen -> ContactsScreen(
                    session,
                    onBack = { navigation.contactsOpen = false },
                    onContactAdded = { navigation.contactsOpen = false },
                    prepareLocalNetwork = prepareLocalNetwork,
                )
                navigation.settingsOpen -> SettingsScreen(
                    session,
                    security,
                    preferences,
                    onBack = { navigation.settingsOpen = false },
                    pickAvatar = pickAvatar,
                )
                chat.conversationId == null -> ChatListPane(
                    conversations = conversations,
                    selected = null,
                    profileName = profile?.displayName,
                    profileAvatar = profile?.avatar,
                    banner = banner,
                    onSelect = session::select,
                    onOpenSettings = { navigation.settingsOpen = true },
                    onOpenContacts = { navigation.contactsOpen = true },
                    onOpenHost = nodeHost?.let { { navigation.openHost() } },
                    onCreateRoom = session::createRoom,
                    onDismissBanner = session::dismissBanner,
                    modifier = Modifier.fillMaxSize(),
                )
                else -> OpenChat(
                    session,
                    chat,
                    conversations,
                    prefs.sendShortcut,
                    identity?.deviceId,
                    voice,
                    onBack = { session.select(null) },
                    modifier = Modifier.fillMaxSize(),
                )
            }
            return@BoxWithConstraints
        }
        // Open the first conversation so the wide layout is never an empty pane.
        LaunchedEffect(conversations.firstOrNull()?.id, chat.conversationId) {
            if (chat.conversationId == null) conversations.firstOrNull()?.let { session.select(it.id) }
        }
        Row(Modifier.fillMaxSize()) {
            ChatListPane(
                conversations = conversations,
                selected = chat.conversationId,
                profileName = profile?.displayName,
                profileAvatar = profile?.avatar,
                banner = banner,
                onSelect = { id ->
                    navigation.hostOpen = false
                    navigation.contactsOpen = false
                    navigation.settingsOpen = false
                    session.select(id)
                },
                onOpenSettings = {
                    navigation.hostOpen = false
                    navigation.contactsOpen = false
                    navigation.settingsOpen = true
                },
                onOpenContacts = {
                    navigation.hostOpen = false
                    navigation.settingsOpen = false
                    navigation.contactsOpen = true
                },
                onOpenHost = nodeHost?.let { { navigation.openHost() } },
                onCreateRoom = session::createRoom,
                onDismissBanner = session::dismissBanner,
                modifier = Modifier.width(340.dp).fillMaxHeight(),
            )
            VerticalDivider()
            Box(Modifier.weight(1f).fillMaxHeight()) {
                when {
                    nodeHost != null && navigation.hostOpen -> HostNodeScreen(nodeHost, onBack = { navigation.hostOpen = false })
                    navigation.contactsOpen -> ContactsScreen(
                        session,
                        onBack = { navigation.contactsOpen = false },
                        onContactAdded = { navigation.contactsOpen = false },
                        prepareLocalNetwork = prepareLocalNetwork,
                    )
                    navigation.settingsOpen -> SettingsScreen(
                        session,
                        security,
                        preferences,
                        onBack = { navigation.settingsOpen = false },
                        pickAvatar = pickAvatar,
                    )
                    else -> OpenChat(
                        session,
                        chat,
                        conversations,
                        prefs.sendShortcut,
                        identity?.deviceId,
                        voice,
                        onBack = null,
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            }
        }
    }
}

@Composable
private fun OpenChat(
    session: ChatSession,
    chat: ChatState,
    conversations: List<com.orbit.sdk.model.Conversation>,
    sendShortcut: com.orbit.client.app.SendShortcut,
    ownDevice: com.orbit.sdk.model.DeviceId?,
    voice: VoiceHost?,
    onBack: (() -> Unit)?,
    modifier: Modifier,
) {
    ChatPane(
        state = chat,
        conversation = conversations.firstOrNull { it.id == chat.conversationId },
        sendShortcut = sendShortcut,
        ownDevice = ownDevice,
        onSend = session::send,
        onEdit = session::edit,
        onDelete = session::delete,
        onLoadOlder = session::loadOlder,
        onDismissError = session::dismissError,
        onBack = onBack,
        voice = voice,
        modifier = modifier,
    )
}
