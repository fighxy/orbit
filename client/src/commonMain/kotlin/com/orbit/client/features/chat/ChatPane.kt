package com.orbit.client.features.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
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
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.orbit.client.designsystem.Strings
import com.orbit.client.designsystem.formatClockTime
import com.orbit.client.features.chatlist.title
import com.orbit.sdk.model.Conversation
import com.orbit.sdk.model.ConversationKind
import com.orbit.sdk.model.Message
import com.orbit.sdk.model.MessageBody
import com.orbit.sdk.model.MessageState
import kotlinx.coroutines.flow.distinctUntilChanged

@Composable
fun ChatPane(
    state: ChatState,
    conversation: Conversation?,
    onSend: (String) -> Unit,
    onLoadOlder: () -> Unit,
    onDismissError: () -> Unit,
    onBack: (() -> Unit)?,
    modifier: Modifier = Modifier,
) {
    if (state.conversationId == null || conversation == null) {
        Box(modifier, contentAlignment = Alignment.Center) {
            Text(Strings.selectChat, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        return
    }
    Column(modifier) {
        ChatHeader(conversation, onBack)
        HorizontalDivider()
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
        Box(Modifier.weight(1f).fillMaxWidth().background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.35f))) {
            when {
                state.loading -> CircularProgressIndicator(Modifier.align(Alignment.Center))
                state.messages.isEmpty() -> EmptyChat(Modifier.align(Alignment.Center))
                else -> MessageList(state, onLoadOlder)
            }
        }
        HorizontalDivider()
        Composer(sending = state.sending > 0, onSend = onSend)
    }
}

@Composable
private fun ChatHeader(conversation: Conversation, onBack: (() -> Unit)?) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp),
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
    val newestFirst = remember(state.messages) { state.messages.asReversed() }
    val currentState by rememberUpdatedState(state)
    val loadOlder by rememberUpdatedState(onLoadOlder)

    // reverseLayout: index 0 is the newest message at the bottom.
    LaunchedEffect(listState) {
        snapshotFlow { listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }
            .distinctUntilChanged()
            .collect { lastVisible ->
                if (currentState.hasMore && lastVisible >= currentState.messages.size - PREFETCH_DISTANCE) loadOlder()
            }
    }
    // Follow new messages when the user is at the bottom.
    LaunchedEffect(newestFirst.firstOrNull()?.id) {
        if (listState.firstVisibleItemIndex <= 1) listState.animateScrollToItem(0)
    }

    LazyColumn(
        state = listState,
        reverseLayout = true,
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 12.dp),
        // Anchor a short history to the composer, like other messengers.
        verticalArrangement = Arrangement.spacedBy(6.dp, Alignment.Bottom),
        modifier = Modifier.fillMaxSize(),
    ) {
        items(newestFirst, key = { it.id.hex }) { message -> MessageBubble(message) }
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
}

@Composable
private fun MessageBubble(message: Message) {
    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
        Surface(
            color = MaterialTheme.colorScheme.primaryContainer,
            shape = RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp, bottomStart = 16.dp, bottomEnd = 4.dp),
            modifier = Modifier.widthIn(max = 520.dp),
        ) {
            Column(Modifier.padding(start = 12.dp, end = 10.dp, top = 8.dp, bottom = 6.dp)) {
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
                        color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.7f),
                    )
                    when (message.state) {
                        MessageState.SavedLocally -> Icon(
                            Icons.Filled.Check,
                            contentDescription = Strings.savedLocally,
                            modifier = Modifier.size(14.dp),
                            tint = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.7f),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun Composer(sending: Boolean, onSend: (String) -> Unit) {
    var text by rememberSaveable { mutableStateOf("") }
    val canSend = text.isNotBlank()
    fun submit() {
        if (!canSend) return
        onSend(text)
        text = ""
    }
    Row(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OutlinedTextField(
            value = text,
            onValueChange = { text = it },
            placeholder = { Text(Strings.composerPlaceholder) },
            maxLines = 6,
            modifier = Modifier
                .weight(1f)
                // Enter sends; Shift+Enter inserts a new line (hardware keyboards).
                .onPreviewKeyEvent { event ->
                    if (event.key == Key.Enter && !event.isShiftPressed) {
                        if (event.type == KeyEventType.KeyDown) submit()
                        true
                    } else {
                        false
                    }
                },
        )
        Spacer(Modifier.width(8.dp))
        Box(contentAlignment = Alignment.Center) {
            IconButton(onClick = ::submit, enabled = canSend, modifier = Modifier.semantics { contentDescription = Strings.send }) {
                Icon(Icons.AutoMirrored.Filled.Send, contentDescription = null)
            }
            if (sending) CircularProgressIndicator(Modifier.size(36.dp), strokeWidth = 2.dp)
        }
    }
}

private const val PREFETCH_DISTANCE = 5
