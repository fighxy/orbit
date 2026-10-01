package com.orbit.client.features.chatlist

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.orbit.client.designsystem.Avatar
import com.orbit.client.designsystem.Strings
import com.orbit.client.designsystem.formatClockTime
import com.orbit.client.designsystem.rememberAvatarImage
import com.orbit.sdk.model.Conversation
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.ConversationKind
import com.orbit.sdk.model.MessageBody

@Composable
fun ChatListPane(
    conversations: List<Conversation>,
    profileName: String?,
    profileAvatar: String? = null,
    selected: ConversationId?,
    banner: String?,
    onSelect: (ConversationId) -> Unit,
    onOpenSettings: () -> Unit,
    onOpenContacts: () -> Unit,
    onOpenHost: (() -> Unit)? = null,
    onCreateRoom: (ConversationKind, String, List<ConversationId>) -> Unit,
    onDismissBanner: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var draftKind by remember { mutableStateOf<ConversationKind?>(null) }
    var draftTitle by remember { mutableStateOf("") }
    var draftMembers by remember { mutableStateOf(setOf<String>()) }
    var query by rememberSaveable { mutableStateOf("") }
    val ready = conversations.filter { it.kind == ConversationKind.Direct && it.contact?.ready == true }
    val needle = query.trim()
    val shown = if (needle.isEmpty()) {
        conversations
    } else {
        conversations.filter { conversation ->
            conversation.title().contains(needle, ignoreCase = true) ||
                conversation.preview().contains(needle, ignoreCase = true)
        }
    }
    val selfImage = rememberAvatarImage(profileAvatar)
    Column(modifier.background(MaterialTheme.colorScheme.surfaceContainer)) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Avatar(
                profileName,
                size = 40.dp,
                image = selfImage,
                modifier = Modifier.clip(CircleShape).clickable(onClick = onOpenSettings),
            )
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(Strings.chats, style = MaterialTheme.typography.titleLarge)
                profileName?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            IconButton(onClick = onOpenSettings) {
                Icon(Icons.Filled.Settings, contentDescription = Strings.settings)
            }
        }
        if (banner != null) {
            Surface(
                color = MaterialTheme.colorScheme.errorContainer,
                modifier = Modifier.fillMaxWidth().clickable(onClick = onDismissBanner),
            ) {
                Text(
                    banner,
                    color = MaterialTheme.colorScheme.onErrorContainer,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(12.dp),
                )
            }
        }
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            placeholder = { Text(Strings.searchChats) },
            singleLine = true,
            shape = RoundedCornerShape(20.dp),
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
        )
        FlowRow(
            Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            AssistChip(onClick = onOpenContacts, label = { Text(Strings.contactsChip) })
            AssistChip(onClick = {
                draftTitle = ""
                draftMembers = emptySet()
                draftKind = ConversationKind.Group
            }, label = { Text(Strings.newGroup) })
            AssistChip(onClick = {
                draftTitle = ""
                draftMembers = emptySet()
                draftKind = ConversationKind.Channel
            }, label = { Text(Strings.newChannel) })
            if (onOpenHost != null) {
                AssistChip(onClick = onOpenHost, label = { Text(Strings.becomeNode) })
            }
        }
        HorizontalDivider()
        if (needle.isNotEmpty() && shown.isEmpty()) {
            Text(
                Strings.searchChatsEmpty,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(16.dp),
            )
        }
        LazyColumn(Modifier.weight(1f)) {
            items(shown, key = { it.id.hex }) { conversation ->
                ConversationRow(
                    conversation = conversation,
                    selected = conversation.id == selected,
                    onClick = { onSelect(conversation.id) },
                )
            }
        }
    }
    val kind = draftKind
    if (kind != null) {
        AlertDialog(
            onDismissRequest = {
                draftKind = null
                draftTitle = ""
                draftMembers = emptySet()
            },
            title = { Text(if (kind == ConversationKind.Channel) Strings.newChannel else Strings.newGroup) },
            text = {
                Column(
                    Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    OutlinedTextField(
                        value = draftTitle,
                        onValueChange = { draftTitle = it },
                        label = { Text(Strings.roomTitle) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    if (ready.isEmpty()) {
                        Text(Strings.roomNeedsContact)
                    } else {
                        ready.forEach { conversation ->
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(
                                    checked = conversation.id.hex in draftMembers,
                                    onCheckedChange = { checked ->
                                        draftMembers = if (checked) draftMembers + conversation.id.hex else draftMembers - conversation.id.hex
                                    },
                                )
                                Text(conversation.title(), maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(
                    enabled = draftTitle.isNotBlank() && draftMembers.isNotEmpty(),
                    onClick = {
                        onCreateRoom(kind, draftTitle, ready.filter { it.id.hex in draftMembers }.map { it.id })
                        draftKind = null
                        draftTitle = ""
                        draftMembers = emptySet()
                    },
                ) { Text(Strings.createRoom) }
            },
            dismissButton = {
                TextButton(onClick = {
                    draftKind = null
                    draftTitle = ""
                    draftMembers = emptySet()
                }) { Text(Strings.cancelEdit) }
            },
        )
    }
}

@Composable
private fun ConversationRow(conversation: Conversation, selected: Boolean, onClick: () -> Unit) {
    val background = if (selected) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp, vertical = 2.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(background)
            .clickable(onClick = onClick)
            .padding(horizontal = 8.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (conversation.kind == ConversationKind.SavedMessages) {
            Box(
                modifier = Modifier.size(48.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary),
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.Filled.Star, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimary)
            }
        } else {
            val picture = if (conversation.kind == ConversationKind.Direct) conversation.contact?.avatar else null
            Avatar(conversation.title(), size = 48.dp, image = rememberAvatarImage(picture))
        }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    conversation.title(),
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.weight(1f),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                conversation.lastMessage?.let {
                    Text(
                        formatClockTime(it.createdAtMs),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Text(
                conversation.preview(),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            val kindLabel = when (conversation.kind) {
                ConversationKind.Group -> Strings.groupSubtitle
                ConversationKind.Channel -> Strings.channelSubtitle
                ConversationKind.SavedMessages -> Strings.savedMessagesSubtitle
                ConversationKind.Direct -> if (conversation.contact?.ready == false) Strings.contactPending else null
            }
            if (kindLabel != null) {
                Text(
                    kindLabel,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.primary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

fun Conversation.title(): String = when (kind) {
    ConversationKind.SavedMessages -> Strings.savedMessages
    ConversationKind.Direct -> contact?.displayName ?: Strings.contact
    ConversationKind.Group -> title ?: Strings.groupSubtitle
    ConversationKind.Channel -> title ?: Strings.channelSubtitle
}

private fun Conversation.preview(): String = when (val body = lastMessage?.body) {
    is MessageBody.Text -> body.text.lineSequence().first()
    MessageBody.Deleted -> Strings.messageDeleted
    is MessageBody.VoiceNote -> Strings.voiceNote
    null -> Strings.noMessagesYet
}
