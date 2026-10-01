package com.orbit.client.features.chat

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.orbit.client.app.SendShortcut
import com.orbit.client.designsystem.Strings
import com.orbit.client.designsystem.formatClockTime
import com.orbit.client.designsystem.formatDayLabel
import com.orbit.client.designsystem.localDayIndex
import com.orbit.client.features.chatlist.title
import com.orbit.sdk.model.Conversation
import com.orbit.sdk.model.ConversationKind
import com.orbit.sdk.model.MessageBody
import com.orbit.sdk.model.MessageState
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
    onSend: (String) -> Unit,
    onLoadOlder: () -> Unit,
    onDismissError: () -> Unit,
    onBack: (() -> Unit)?,
    modifier: Modifier = Modifier,
) {
    if (state.conversationId == null || conversation == null) {
        Box(modifier.background(MaterialTheme.colorScheme.surfaceContainerLow), contentAlignment = Alignment.Center) {
            Text(Strings.selectChat, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        return
    }
    Column(modifier) {
        ChatHeader(conversation, onBack)
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
                else -> MessageList(state, onLoadOlder)
            }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
        Composer(sending = state.sending > 0, sendShortcut = sendShortcut, onSend = onSend)
    }
}

@Composable
private fun ChatHeader(conversation: Conversation, onBack: (() -> Unit)?) {
    Row(
        modifier = Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (onBack != null) {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = Strings.back)
            }
        } else {
            Spacer(Modifier.width(8.dp))
        }
        Column {
            Text(conversation.title(), style = MaterialTheme.typography.titleMedium)
            if (conversation.kind == ConversationKind.SavedMessages) {
                Text(
                    Strings.savedMessagesSubtitle,
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
private fun MessageList(state: ChatState, onLoadOlder: () -> Unit) {
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
                    is TimelineItem.Entry -> MessageBubble(row)
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
private fun MessageBubble(entry: TimelineItem.Entry) {
    val message = entry.message
    // Own messages sit on the right. Corners facing a neighbour in the same
    // group are tight; the group's last bubble gets the sharpest "tail" corner.
    val shape = RoundedCornerShape(
        topStart = 18.dp,
        topEnd = if (entry.firstInGroup) 18.dp else 6.dp,
        bottomStart = 18.dp,
        bottomEnd = if (entry.lastInGroup) 4.dp else 6.dp,
    )
    Box(
        Modifier.fillMaxWidth().padding(top = if (entry.firstInGroup) 6.dp else 2.dp),
        contentAlignment = Alignment.CenterEnd,
    ) {
        Surface(
            color = MaterialTheme.colorScheme.primaryContainer,
            shape = shape,
            modifier = Modifier.widthIn(max = 560.dp),
        ) {
            Column(Modifier.padding(start = 12.dp, end = 10.dp, top = 7.dp, bottom = 5.dp)) {
                when (val body = message.body) {
                    is MessageBody.Text -> SelectionContainer {
                        Text(
                            body.text,
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onPrimaryContainer,
                        )
                    }
                }
                Row(
                    modifier = Modifier.align(Alignment.End),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text(
                        formatClockTime(message.createdAtMs),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.65f),
                    )
                    when (message.state) {
                        MessageState.SavedLocally -> Icon(
                            Icons.Filled.Check,
                            contentDescription = Strings.savedLocally,
                            modifier = Modifier.size(14.dp),
                            tint = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.65f),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun Composer(sending: Boolean, sendShortcut: SendShortcut, onSend: (String) -> Unit) {
    var text by rememberSaveable { mutableStateOf("") }
    val bytes = remember(text) { text.encodeToByteArray().size }
    val tooLong = bytes > MAX_TEXT_BYTES
    val canSend = text.isNotBlank() && !tooLong
    fun submit() {
        if (!canSend) return
        onSend(text)
        text = ""
    }
    Column(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surface)) {
        if (bytes > MAX_TEXT_BYTES * 9 / 10) {
            Text(
                if (tooLong) "${Strings.messageTooLong}: $bytes / $MAX_TEXT_BYTES" else "$bytes / $MAX_TEXT_BYTES",
                style = MaterialTheme.typography.labelSmall,
                color = if (tooLong) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 20.dp, top = 6.dp),
            )
        }
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            TextField(
                value = text,
                onValueChange = { text = it },
                placeholder = { Text(Strings.composerPlaceholder) },
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
                IconButton(onClick = ::submit, enabled = canSend) {
                    Icon(
                        Icons.AutoMirrored.Filled.Send,
                        contentDescription = Strings.send,
                        tint = if (canSend) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (sending) CircularProgressIndicator(Modifier.size(40.dp), strokeWidth = 2.dp)
            }
        }
    }
}
