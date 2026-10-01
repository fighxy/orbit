package com.orbit.client.features.contacts

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.orbit.client.designsystem.Strings
import com.orbit.client.features.chat.ChatSession
import com.orbit.sdk.OrbitException
import com.orbit.sdk.model.ConnectionState
import com.orbit.sdk.model.InvitePreview
import kotlinx.coroutines.launch

/** Common flow on Windows, Android and iOS; the Rust core owns registration and trust checks. */
@Composable
fun ContactsScreen(
    session: ChatSession,
    onBack: () -> Unit,
    onContactAdded: () -> Unit,
    prepareLocalNetwork: (suspend () -> Boolean)? = null,
    localNetworkHint: String = Strings.localNetworkHint,
) {
    val network by session.network.collectAsState()
    val scope = rememberCoroutineScope()
    var node by rememberSaveable { mutableStateOf(network.node.orEmpty()) }
    // Registration codes and bearer invitations are not saved into UI state bundles.
    var code by remember { mutableStateOf("") }
    var invitation by remember { mutableStateOf("") }
    var ownInvitation by remember { mutableStateOf<String?>(null) }
    var preview by remember { mutableStateOf<InvitePreview?>(null) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var permissionNote by remember { mutableStateOf<String?>(null) }
    var showMailbox by rememberSaveable { mutableStateOf(false) }
    val onMailbox = network.node?.contains('@') == true
    LaunchedEffect(network.node) {
        val current = network.node.orEmpty()
        if (node.isBlank() && current.contains('@')) node = current
    }

    fun perform(action: suspend () -> Unit) {
        busy = true
        error = null
        scope.launch {
            try { action() } catch (e: OrbitException) { error = Strings.describe(e) }
            finally { busy = false }
        }
    }

    suspend fun prepareNetwork() {
        prepareLocalNetwork?.let { request ->
            permissionNote = if (request()) null else Strings.localNetworkDenied
        }
    }

    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(12.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            TextButton(onClick = onBack) { Text(Strings.back) }
            Text(Strings.contacts, style = MaterialTheme.typography.headlineSmall)
        }
        HorizontalDivider()
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(20.dp).widthIn(max = 680.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)) {
            if (!onMailbox) {
                Text(Strings.directTitle, style = MaterialTheme.typography.titleLarge)
                Text(Strings.directHint)
                Text(when (network.state) {
                    ConnectionState.Unconfigured -> Strings.directNeedsProfile
                    ConnectionState.Connecting -> Strings.directConnecting
                    ConnectionState.Online -> Strings.directOnline
                    ConnectionState.Offline -> Strings.directOffline
                }, color = MaterialTheme.colorScheme.primary)
                if (!showMailbox) {
                    TextButton(onClick = { showMailbox = true }) { Text(Strings.useOwnNode) }
                }
            }
            if (onMailbox || showMailbox) {
                Text(Strings.deliveryServer, style = MaterialTheme.typography.titleLarge)
                Text(Strings.deliveryServerHint)
                if (onMailbox) {
                    Text(when (network.state) {
                        ConnectionState.Unconfigured -> Strings.serverNotConfigured
                        ConnectionState.Connecting -> Strings.serverConnecting
                        ConnectionState.Online -> Strings.serverConnected
                        ConnectionState.Offline -> Strings.serverOffline
                    }, color = MaterialTheme.colorScheme.primary)
                }
                OutlinedTextField(node, { node = it }, label = { Text(Strings.nodeAddress) },
                    enabled = !busy, modifier = Modifier.fillMaxWidth(), maxLines = 3)
                OutlinedTextField(code, { code = it }, label = { Text(Strings.registrationCode) },
                    enabled = !busy, singleLine = true, visualTransformation = PasswordVisualTransformation(), modifier = Modifier.fillMaxWidth())
                prepareLocalNetwork?.let { Text(localNetworkHint, style = MaterialTheme.typography.bodySmall) }
                Button(onClick = { perform { prepareNetwork(); session.registerNode(node.trim(), code.takeIf { it.isNotBlank() }); code = "" } }, enabled = !busy && node.isNotBlank()) {
                    Text(Strings.registerOnServer)
                }
            }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            (error ?: network.error)?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            permissionNote?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            HorizontalDivider()
            Text(Strings.myInvitation, style = MaterialTheme.typography.titleLarge)
            Text(Strings.invitationHint)
            Button(onClick = { perform { ownInvitation = session.createInvite() } }, enabled = !busy && network.state == ConnectionState.Online) {
                Text(Strings.createInvitation)
            }
            ownInvitation?.let { link ->
                Text(Strings.copyInvitationHint, style = MaterialTheme.typography.bodySmall)
                SelectionContainer { Text(link, style = MaterialTheme.typography.bodySmall) }
            }
            HorizontalDivider()
            Text(Strings.addContact, style = MaterialTheme.typography.titleLarge)
            OutlinedTextField(invitation, { invitation = it; preview = null }, label = { Text(Strings.pasteInvitation) },
                enabled = !busy, maxLines = 5, modifier = Modifier.fillMaxWidth())
            Button(onClick = { perform { preview = session.inspectInvite(invitation) } }, enabled = !busy && invitation.isNotBlank()) {
                Text(Strings.verifyInvitation)
            }
            preview?.let { contact ->
                Text(contact.displayName, style = MaterialTheme.typography.titleMedium)
                Text(Strings.verifyContactKey)
                SelectionContainer { Text(contact.accountId.hex.uppercase().chunked(4).joinToString(" "), style = MaterialTheme.typography.bodySmall) }
                Button(onClick = { perform { prepareNetwork(); session.acceptInvite(invitation); invitation = ""; preview = null; onContactAdded() } },
                    enabled = !busy && network.state == ConnectionState.Online) { Text(Strings.addContact) }
            }
        }
    }
}
