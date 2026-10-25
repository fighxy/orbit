package com.orbit.client.features.chat

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SmallFloatingActionButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.orbit.client.app.SendShortcut
import com.orbit.client.designsystem.Avatar
import com.orbit.client.designsystem.Strings
import com.orbit.client.designsystem.rememberAvatarImage
import com.orbit.client.designsystem.formatClockTime
import com.orbit.client.designsystem.formatDayLabel
import com.orbit.client.designsystem.localDayIndex
import com.orbit.client.features.chatlist.title
import com.orbit.sdk.model.Conversation
import com.orbit.sdk.model.ConversationKind
import com.orbit.sdk.model.MessageBody
import com.orbit.sdk.model.MessageId
import com.orbit.sdk.model.MessageState
import com.orbit.sdk.model.DeviceId
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch

/** Matches `orbit_core::limits::MAX_TEXT_BYTES`. */
private const val MAX_TEXT_BYTES = 16 * 1024
private const val PREFETCH_DISTANCE = 5

@Composable
fun ChatPane(
    state: ChatState,
    conversation: Conversation?,
    sendShortcut: SendShortcut,
    ownDevice: DeviceId?,
    onSend: (String) -> Unit,
    onEdit: (MessageId, String) -> Unit,
    onDelete: (MessageId) -> Unit,
    onLoadOlder: () -> Unit,
    onDismissError: () -> Unit,
    onBack: (() -> Unit)?,
    voice: VoiceHost? = null,
    modifier: Modifier = Modifier,
) {
    val allowsVoice = voice != null && (
        conversation?.kind == ConversationKind.Direct || conversation?.kind == ConversationKind.SavedMessages
        )
    DisposableEffect(voice, conversation?.id, allowsVoice) {
        onDispose {
            if (allowsVoice && voice != null) voice.commit()
        }
    }
    var editingId by rememberSaveable { mutableStateOf<String?>(null) }
    var menuFor by rememberSaveable { mutableStateOf<String?>(null) }
    var pendingDelete by rememberSaveable { mutableStateOf<String?>(null) }
    LaunchedEffect(conversation?.id?.hex) {
        editingId = null
        menuFor = null
        pendingDelete = null
    }
    if (state.conversationId == null || conversation == null) {
        Box(modifier.background(MaterialTheme.colorScheme.surfaceContainerLow), contentAlignment = Alignment.Center) {
            Text(Strings.selectChat, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        return
    }
    Column(modifier) {
        ChatHeader(conversation, onBack, rememberAvatarImage(conversation.contact?.avatar))
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
        if (state.error != null) {
            Surface(color = MaterialTheme.colorScheme.errorContainer, modifier = Modifier.fillMaxWidth()) {
                Row(Modifier.padding(start = 16.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        state.error,
                        color = MaterialTheme.colorScheme.onErrorContainer,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = onDismissError) { Text(Strings.dismiss) }
                }
            }
        }
        Box(Modifier.weight(1f).fillMaxWidth().background(MaterialTheme.colorScheme.surfaceContainerLow)) {
            when {
                state.loading -> CircularProgressIndicator(Modifier.align(Alignment.Center))
                state.messages.isEmpty() -> EmptyChat(Modifier.align(Alignment.Center))
                else -> MessageList(
                    state,
                    ownDevice,
                    onLoadOlder,
                    menuFor = menuFor,
                    onOpenMenu = { menuFor = it },
                    onDismissMenu = { menuFor = null },
                    onStartEdit = {
                        menuFor = null
                        editingId = it
                    },
                    voice = voice,
                    onAskDelete = {
                        menuFor = null
                        pendingDelete = it
                    },
                )
            }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
        val editing = state.messages.firstOrNull { it.id.hex == editingId && !it.deleted }
        if (!conversation.canPost) {
            Text(
                Strings.channelReadOnly,
                modifier = Modifier.fillMaxWidth().padding(16.dp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else Composer(
            sending = state.sending > 0,
            sendShortcut = sendShortcut,
            placeholder = if (conversation.kind == ConversationKind.SavedMessages) Strings.composerPlaceholder else Strings.composerMessage,
            editing = editing != null,
            initialText = (editing?.body as? MessageBody.Text)?.text.orEmpty(),
            onCancelEdit = { editingId = null },
            voice = if (allowsVoice) voice else null,
            conversationId = conversation.id,
            onSend = { text ->
                if (editing != null) {
                    onEdit(editing.id, text)
                    editingId = null
                } else {
                    onSend(text)
                }
            },
        )
        val deleting = state.messages.firstOrNull { it.id.hex == pendingDelete }
        if (deleting != null) {
            AlertDialog(
                onDismissRequest = { pendingDelete = null },
                title = { Text(Strings.deleteMessageTitle) },
                text = {
                    Text(
                        if (conversation.kind == ConversationKind.Group || conversation.kind == ConversationKind.Channel) {
                            Strings.deleteRoomBody
                        } else {
                            Strings.deleteMessageBody
                        },
                    )
                },
                confirmButton = {
                    TextButton(onClick = {
                        onDelete(deleting.id)
                        if (editingId == deleting.id.hex) editingId = null
                        pendingDelete = null
                    }) { Text(Strings.deleteMessage) }
                },
                dismissButton = {
                    TextButton(onClick = { pendingDelete = null }) { Text(Strings.cancelEdit) }
                },
            )
        }
    }
}

@Composable
private fun ChatHeader(
    conversation: Conversation,
    onBack: (() -> Unit)?,
    avatar: androidx.compose.ui.graphics.ImageBitmap?,
) {
    Row(
        modifier = Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (onBack != null) {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = Strings.back)
            }
        } else {
            Spacer(Modifier.width(8.dp))
        }
        if (conversation.kind != ConversationKind.SavedMessages) {
            Avatar(conversation.title(), size = 36.dp, image = avatar)
            Spacer(Modifier.width(10.dp))
        }
        Column(Modifier.weight(1f)) {
            Text(conversation.title(), style = MaterialTheme.typography.titleMedium)
            if (conversation.contact?.ready == false) {
                Text(Strings.contactPending, style = MaterialTheme.typography.bodySmall)
            }
            if (conversation.kind == ConversationKind.SavedMessages) {
                Text(
                    Strings.savedMessagesSubtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (conversation.kind == ConversationKind.Group || conversation.kind == ConversationKind.Channel) {
                Text(
                    if (conversation.kind == ConversationKind.Channel) Strings.channelSubtitle else Strings.groupSubtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun EmptyChat(modifier: Modifier = Modifier) {
    Column(
        modifier = modifier.widthIn(max = 360.dp).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(Strings.emptyChatTitle, style = MaterialTheme.typography.titleMedium)
        Text(
            Strings.emptyChatBody,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
private fun MessageList(
    state: ChatState,
    ownDevice: DeviceId?,
    onLoadOlder: () -> Unit,
    menuFor: String?,
    onOpenMenu: (String) -> Unit,
    onDismissMenu: () -> Unit,
    onStartEdit: (String) -> Unit,
    onAskDelete: (String) -> Unit,
    voice: VoiceHost?,
) {
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    // reverseLayout: index 0 is the newest row at the bottom.
    val rows = remember(state.messages) {
        buildTimeline(state.messages, ::localDayIndex, { formatDayLabel(it) }).asReversed()
    }
    val currentState by rememberUpdatedState(state)
    val loadOlder by rememberUpdatedState(onLoadOlder)
    val showJumpToLatest by remember { derivedStateOf { listState.firstVisibleItemIndex > 2 } }

    LaunchedEffect(listState) {
        snapshotFlow { listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }
            .distinctUntilChanged()
            .collect { lastVisible ->
                if (currentState.hasMore && lastVisible >= listState.layoutInfo.totalItemsCount - PREFETCH_DISTANCE) {
                    loadOlder()
                }
            }
    }
    // Follow new messages while the user is at the bottom.
    LaunchedEffect(state.messages.lastOrNull()?.id) {
        if (listState.firstVisibleItemIndex <= 1) listState.animateScrollToItem(0)
    }

    Box(Modifier.fillMaxSize()) {
        LazyColumn(
            state = listState,
            reverseLayout = true,
            contentPadding = PaddingValues(horizontal = 12.dp, vertical = 12.dp),
            // Anchors a short history to the composer.
            verticalArrangement = Arrangement.Bottom,
            modifier = Modifier.fillMaxSize(),
        ) {
            items(rows, key = { it.key }) { row ->
                when (row) {
                    is TimelineItem.Day -> DaySeparator(row.label)
                    is TimelineItem.Entry -> MessageBubble(
                        row,
                        own = row.message.authorDevice == ownDevice,
                        menuOpen = menuFor == row.message.id.hex,
                        onOpenMenu = onOpenMenu,
                        onDismissMenu = onDismissMenu,
                        onStartEdit = onStartEdit,
                        onAskDelete = onAskDelete,
                        voice = voice,
                    )
                }
            }
            if (state.loadingOlder) {
                item(key = "loading-older") {
                    Text(
                        Strings.loadingOlder,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.fillMaxWidth().padding(8.dp),
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
        AnimatedVisibility(
            visible = showJumpToLatest,
            enter = fadeIn() + scaleIn(),
            exit = fadeOut() + scaleOut(),
            modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp),
        ) {
            SmallFloatingActionButton(
                onClick = { scope.launch { listState.animateScrollToItem(0) } },
                containerColor = MaterialTheme.colorScheme.surfaceContainerHighest,
            ) {
                Icon(Icons.Filled.KeyboardArrowDown, contentDescription = Strings.scrollToLatest)
            }
        }
    }
}

@Composable
private fun DaySeparator(label: String) {
    Box(Modifier.fillMaxWidth().padding(vertical = 8.dp), contentAlignment = Alignment.Center) {
        Surface(
            color = MaterialTheme.colorScheme.surfaceContainerHighest,
            shape = RoundedCornerShape(50),
        ) {
            Text(
                label,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
            )
        }
    }
}

@Composable
private fun MessageBubble(
    entry: TimelineItem.Entry,
    own: Boolean,
    menuOpen: Boolean,
    onOpenMenu: (String) -> Unit,
    onDismissMenu: () -> Unit,
    onStartEdit: (String) -> Unit,
    onAskDelete: (String) -> Unit,
    voice: VoiceHost?,
) {
    val message = entry.message
    val canEdit = own && message.body is MessageBody.Text && !message.deleted
    val canDelete = own && !message.deleted && message.body !is MessageBody.Deleted
    // Own messages sit on the right. Corners facing a neighbour in the same
    // group are tight; the group's last bubble gets the sharpest "tail" corner.
    val shape = RoundedCornerShape(
        topStart = if (own || entry.firstInGroup) 18.dp else 6.dp,
        topEnd = if (!own || entry.firstInGroup) 18.dp else 6.dp,
        bottomStart = if (own) 18.dp else if (entry.lastInGroup) 4.dp else 6.dp,
        bottomEnd = if (!own) 18.dp else if (entry.lastInGroup) 4.dp else 6.dp,
    )
    Box(
        Modifier.fillMaxWidth().padding(top = if (entry.firstInGroup) 6.dp else 2.dp),
        contentAlignment = if (own) Alignment.CenterEnd else Alignment.CenterStart,
    ) {
        Surface(
            color = if (own) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
            shape = shape,
            modifier = Modifier.widthIn(max = 560.dp).then(
                if (canDelete) Modifier.onSecondaryPress { onOpenMenu(message.id.hex) } else Modifier,
            ),
        ) {
            Column(Modifier.padding(start = 12.dp, end = 8.dp, top = 7.dp, bottom = 2.dp)) {
                when (val body = message.body) {
                    is MessageBody.Text -> SelectionContainer {
                        Text(
                            body.text,
                            style = MaterialTheme.typography.bodyLarge,
                            color = if (own) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface,
                        )
                    }
                    MessageBody.Deleted -> Text(
                        Strings.messageDeleted,
                        style = MaterialTheme.typography.bodyMedium,
                        fontStyle = FontStyle.Italic,
                        color = (if (own) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface).copy(alpha = 0.7f),
                    )
                    is MessageBody.VoiceNote -> {
                        val ink = if (own) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface
                        val playing = voice?.playingHex == message.id.hex
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            if (voice != null) {
                                IconButton(
                                    onClick = { voice.togglePlay(message.id) },
                                    modifier = Modifier.size(36.dp),
                                ) {
                                    if (playing) StopGlyph(ink) else PlayGlyph(ink)
                                }
                            }
                            Column {
                                Text(
                                    "${Strings.voiceNote} · ${Strings.voiceClock(body.durationMs)}",
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = ink,
                                )
                                if (body.waveform.isNotEmpty()) {
                                    Row(
                                        modifier = Modifier.padding(top = 6.dp).height(28.dp),
                                        horizontalArrangement = Arrangement.spacedBy(2.dp),
                                        verticalAlignment = Alignment.Bottom,
                                    ) {
                                        body.waveform.forEach { amp ->
                                            val bar = (4 + amp.coerceIn(0, 255) * 24 / 255).dp
                                            Box(
                                                Modifier
                                                    .width(3.dp)
                                                    .height(bar)
                                                    .background(ink.copy(alpha = if (playing) 1f else 0.85f), RoundedCornerShape(2.dp)),
                                            )
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Row(
                    modifier = Modifier.align(Alignment.End),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    if (message.editedAtMs != null && !message.deleted) {
                        Text(
                            Strings.messageEdited,
                            style = MaterialTheme.typography.labelSmall,
                            color = (if (own) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface).copy(alpha = 0.65f),
                        )
                    }
                    Text(
                        formatClockTime(message.createdAtMs),
                        style = MaterialTheme.typography.labelSmall,
                        color = (if (own) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface).copy(alpha = 0.65f),
                    )
                    if (canDelete) {
                        Box {
                            IconButton(onClick = { onOpenMenu(message.id.hex) }, modifier = Modifier.size(28.dp)) {
                                Icon(
                                    Icons.Filled.MoreVert,
                                    contentDescription = Strings.messageActions,
                                    modifier = Modifier.size(16.dp),
                                    tint = (if (own) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface).copy(alpha = 0.65f),
                                )
                            }
                            DropdownMenu(expanded = menuOpen, onDismissRequest = onDismissMenu) {
                                if (canEdit) {
                                    DropdownMenuItem(
                                        text = { Text(Strings.editMessage) },
                                        onClick = { onStartEdit(message.id.hex) },
                                    )
                                }
                                DropdownMenuItem(
                                    text = { Text(Strings.deleteMessage) },
                                    onClick = { onAskDelete(message.id.hex) },
                                )
                            }
                        }
                    }
                    if (own) Text(when (message.state) {
                        MessageState.SavedLocally -> Strings.savedLocally
                        MessageState.Queued -> Strings.messageQueued
                        MessageState.Mailbox -> Strings.messageOnServer
                        MessageState.Delivered -> Strings.messageDelivered
                        MessageState.Received -> ""
                    }, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
    }
}

@Composable
private fun Composer(
    sending: Boolean,
    sendShortcut: SendShortcut,
    placeholder: String,
    editing: Boolean,
    initialText: String,
    onCancelEdit: () -> Unit,
    voice: VoiceHost?,
    conversationId: com.orbit.sdk.model.ConversationId,
    onSend: (String) -> Unit,
) {
    var text by rememberSaveable(initialText) { mutableStateOf(initialText) }
    val bytes = remember(text) { text.encodeToByteArray().size }
    val tooLong = bytes > MAX_TEXT_BYTES
    val canSend = text.isNotBlank() && !tooLong
    fun submit() {
        if (!canSend) return
        onSend(text)
        text = ""
    }
    Column(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surface)) {
        if (editing) {
            Row(
                Modifier.fillMaxWidth().padding(start = 20.dp, end = 8.dp, top = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    Strings.editingMessage,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = onCancelEdit) { Text(Strings.cancelEdit) }
            }
        }
        if (bytes > MAX_TEXT_BYTES * 9 / 10) {
            Text(
                if (tooLong) "${Strings.messageTooLong}: $bytes / $MAX_TEXT_BYTES" else "$bytes / $MAX_TEXT_BYTES",
                style = MaterialTheme.typography.labelSmall,
                color = if (tooLong) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 20.dp, top = 6.dp),
            )
        }
        val voiceError = voice?.error
        if (voice != null && voiceError != null) {
            Row(
                Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    voiceError,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = voice::dismissError) { Text(Strings.dismiss) }
            }
        }
        val recording = voice?.recording == true
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            if (recording) {
                Surface(
                    color = MaterialTheme.colorScheme.primaryContainer,
                    shape = RoundedCornerShape(24.dp),
                    modifier = Modifier.weight(1f).height(56.dp),
                ) {
                    Row(
                        Modifier.fillMaxSize().padding(horizontal = 16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(
                            Modifier.size(10.dp).background(MaterialTheme.colorScheme.error, CircleShape),
                        )
                        Spacer(Modifier.width(10.dp))
                        Text(
                            "${Strings.voiceClock(voice.elapsedMs)} · 1:00",
                            style = MaterialTheme.typography.titleMedium,
                            color = MaterialTheme.colorScheme.onPrimaryContainer,
                        )
                    }
                }
            } else TextField(
                value = text,
                onValueChange = { text = it },
                placeholder = { Text(placeholder) },
                maxLines = 8,
                shape = RoundedCornerShape(24.dp),
                colors = TextFieldDefaults.colors(
                    focusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent,
                    unfocusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent,
                    focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                    unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                ),
                modifier = Modifier
                    .weight(1f)
                    .onPreviewKeyEvent { event ->
                        if (event.key != Key.Enter || event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                        val modifierPressed = event.isCtrlPressed || event.isMetaPressed
                        val sends = when (sendShortcut) {
                            SendShortcut.Enter -> !event.isShiftPressed && !modifierPressed
                            SendShortcut.CtrlEnter -> modifierPressed
                        }
                        if (sends) submit()
                        sends
                    },
            )
            Spacer(Modifier.width(8.dp))
            Box(Modifier.size(56.dp), contentAlignment = Alignment.Center) {
                val showMic = voice != null && !editing && text.isBlank()
                when {
                    recording -> IconButton(onClick = { voice.commit() }) {
                        StopGlyph(MaterialTheme.colorScheme.error)
                    }
                    showMic -> IconButton(onClick = { voice.toggleRecord(conversationId) }) {
                        MicGlyph(MaterialTheme.colorScheme.primary)
                    }
                    else -> IconButton(onClick = ::submit, enabled = canSend) {
                        Icon(
                            Icons.AutoMirrored.Filled.Send,
                            contentDescription = Strings.send,
                            tint = if (canSend) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                if (sending && !recording) CircularProgressIndicator(Modifier.size(40.dp), strokeWidth = 2.dp)
            }
        }
    }
}

@Composable
private fun PlayGlyph(color: Color) {
    Canvas(Modifier.size(18.dp)) {
        val path = Path().apply {
            moveTo(size.width * 0.28f, size.height * 0.16f)
            lineTo(size.width * 0.82f, size.height * 0.5f)
            lineTo(size.width * 0.28f, size.height * 0.84f)
            close()
        }
        drawPath(path, color)
    }
}

@Composable
private fun StopGlyph(color: Color) {
    Canvas(Modifier.size(16.dp)) {
        drawRoundRect(color, cornerRadius = CornerRadius(3.dp.toPx(), 3.dp.toPx()))
    }
}

@Composable
private fun MicGlyph(color: Color) {
    Canvas(Modifier.size(22.dp)) {
        val body = Size(size.width * 0.36f, size.height * 0.46f)
        drawRoundRect(
            color = color,
            topLeft = Offset((size.width - body.width) / 2f, size.height * 0.06f),
            size = body,
            cornerRadius = CornerRadius(body.width / 2f, body.width / 2f),
        )
        drawArc(
            color = color,
            startAngle = 0f,
            sweepAngle = 180f,
            useCenter = false,
            topLeft = Offset(size.width * 0.18f, size.height * 0.28f),
            size = Size(size.width * 0.64f, size.height * 0.46f),
            style = androidx.compose.ui.graphics.drawscope.Stroke(width = size.width * 0.08f),
        )
        drawLine(
            color,
            Offset(size.width / 2f, size.height * 0.74f),
            Offset(size.width / 2f, size.height * 0.88f),
            strokeWidth = size.width * 0.08f,
        )
        drawLine(
            color,
            Offset(size.width * 0.32f, size.height * 0.88f),
            Offset(size.width * 0.68f, size.height * 0.88f),
            strokeWidth = size.width * 0.08f,
            cap = androidx.compose.ui.graphics.StrokeCap.Round,
        )
    }
}
