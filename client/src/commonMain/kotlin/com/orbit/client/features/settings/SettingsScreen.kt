package com.orbit.client.features.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import com.orbit.client.app.ProfileRules
import com.orbit.client.designsystem.Avatar
import com.orbit.client.designsystem.Strings
import com.orbit.client.features.chat.ChatSession
import com.orbit.sdk.bridge.SUPPORTED_ABI_VERSION
import kotlinx.coroutines.launch

/** Operations that need the account gateway rather than the open session. */
interface SecurityActions {
    val passcodeEnabled: Boolean

    suspend fun setPasscode(currentPasscode: String?, newPasscode: String): String?

    suspend fun removePasscode(currentPasscode: String): String?

    fun lockNow()
}

@Composable
fun SettingsScreen(session: ChatSession, security: SecurityActions, onBack: () -> Unit) {
    val identity by session.identity.collectAsState()
    val profile by session.profile.collectAsState()

    Column(Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = Strings.back) }
            Text(Strings.settings, style = MaterialTheme.typography.titleLarge)
        }
        HorizontalDivider()
        Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState()), contentAlignment = Alignment.TopCenter) {
            Column(
                modifier = Modifier.widthIn(max = 640.dp).fillMaxWidth().padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                ProfileSection(session, profile?.displayName.orEmpty(), profile?.about.orEmpty())
                Section(Strings.accountSection) {
                    identity?.let { id ->
                        Text(Strings.accountKey, style = MaterialTheme.typography.labelMedium)
                        Text(id.accountFingerprint, style = MaterialTheme.typography.titleMedium)
                        Text(Strings.accountKeyFull, style = MaterialTheme.typography.labelMedium)
                        SelectionContainer {
                            Text(
                                id.accountId.hex.chunked(8).joinToString(" "),
                                fontFamily = FontFamily.Monospace,
                                style = MaterialTheme.typography.bodySmall,
                            )
                        }
                        Text(Strings.deviceKey, style = MaterialTheme.typography.labelMedium)
                        SelectionContainer {
                            Text(
                                id.deviceId.hex.take(16).uppercase().chunked(4).joinToString(" "),
                                fontFamily = FontFamily.Monospace,
                                style = MaterialTheme.typography.bodySmall,
                            )
                        }
                    }
                    Hint(Strings.accountKeyExplanation)
                }
                SecuritySection(security)
                Section(Strings.aboutSection) {
                    Text("${Strings.version}: $APP_VERSION (ABI $SUPPORTED_ABI_VERSION)")
                    Hint(Strings.storageNote)
                    Hint(Strings.networkNote)
                }
            }
        }
    }
}

@Composable
private fun ProfileSection(session: ChatSession, savedName: String, savedAbout: String) {
    var name by rememberSaveable(savedName) { mutableStateOf(savedName) }
    var about by rememberSaveable(savedAbout) { mutableStateOf(savedAbout) }
    var status by remember { mutableStateOf<String?>(null) }
    var saving by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val changed = name.trim() != savedName || about.trim() != savedAbout
    val error = ProfileRules.validate(name, about)

    Section(Strings.profileSection) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Avatar(name.ifBlank { null }, size = 64.dp)
            Spacer(Modifier.width(16.dp))
            Column {
                Text(name.ifBlank { "—" }, style = MaterialTheme.typography.titleMedium)
                if (about.isNotBlank()) {
                    Text(about, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        OutlinedTextField(
            value = name,
            onValueChange = { if (it.length <= ProfileRules.MAX_NAME) name = it.replace("\n", "") },
            label = { Text(Strings.displayName) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = about,
            onValueChange = { if (it.length <= ProfileRules.MAX_ABOUT) about = it },
            label = { Text(Strings.about) },
            supportingText = { Text("${about.length} / ${ProfileRules.MAX_ABOUT}") },
            maxLines = 3,
            modifier = Modifier.fillMaxWidth(),
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(
                enabled = changed && error == null && !saving,
                onClick = {
                    saving = true
                    scope.launch {
                        status = session.updateProfile(name, about) ?: Strings.saved
                        saving = false
                    }
                },
            ) { Text(Strings.save) }
            Spacer(Modifier.width(12.dp))
            val message = if (changed) error else status
            message?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
        }
    }
}

private enum class PasscodeDialogMode { Set, Change, Remove }

@Composable
private fun SecuritySection(security: SecurityActions) {
    var dialog by remember { mutableStateOf<PasscodeDialogMode?>(null) }
    Section(Strings.securitySection) {
        Text(Strings.passcode, style = MaterialTheme.typography.labelMedium)
        Text(if (security.passcodeEnabled) Strings.passcodeOn else Strings.passcodeOff)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (security.passcodeEnabled) {
                OutlinedButton(onClick = { dialog = PasscodeDialogMode.Change }) { Text(Strings.changePasscode) }
                OutlinedButton(onClick = { dialog = PasscodeDialogMode.Remove }) { Text(Strings.disablePasscode) }
                Button(onClick = security::lockNow) { Text(Strings.lockNow) }
            } else {
                Button(onClick = { dialog = PasscodeDialogMode.Set }) { Text(Strings.setPasscode) }
            }
        }
        Hint(Strings.passcodeWarning)
    }
    dialog?.let { mode -> PasscodeDialog(mode, security, onDismiss = { dialog = null }) }
}

@Composable
private fun PasscodeDialog(mode: PasscodeDialogMode, security: SecurityActions, onDismiss: () -> Unit) {
    var current by remember { mutableStateOf("") }
    var new by remember { mutableStateOf("") }
    var confirmation by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val needsCurrent = mode != PasscodeDialogMode.Set
    val needsNew = mode != PasscodeDialogMode.Remove
    val title = when (mode) {
        PasscodeDialogMode.Set -> Strings.setPasscode
        PasscodeDialogMode.Change -> Strings.changePasscode
        PasscodeDialogMode.Remove -> Strings.disablePasscode
    }
    LaunchedEffect(mode) { error = null }

    androidx.compose.material3.AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text("${Strings.passcode}: ${title.lowercase()}") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (needsCurrent) {
                    com.orbit.client.designsystem.PasscodeField(current, { current = it }, Strings.currentPasscode, enabled = !busy)
                }
                if (needsNew) {
                    com.orbit.client.designsystem.PasscodeField(new, { new = it }, Strings.newPasscode, enabled = !busy)
                    com.orbit.client.designsystem.PasscodeField(
                        confirmation,
                        { confirmation = it },
                        Strings.confirmPasscode,
                        isError = confirmation.isNotEmpty() && confirmation != new,
                        enabled = !busy,
                    )
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                if (busy) Text(Strings.unlocking, style = MaterialTheme.typography.bodySmall)
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy && (!needsCurrent || current.isNotEmpty()) && (!needsNew || new.isNotEmpty()),
                onClick = {
                    if (needsNew && new != confirmation) {
                        error = Strings.passcodeMismatch
                        return@TextButton
                    }
                    busy = true
                    scope.launch {
                        val result = when (mode) {
                            PasscodeDialogMode.Set -> security.setPasscode(null, new)
                            PasscodeDialogMode.Change -> security.setPasscode(current, new)
                            PasscodeDialogMode.Remove -> security.removePasscode(current)
                        }
                        busy = false
                        if (result == null) onDismiss() else error = result
                    }
                },
            ) { Text(Strings.confirm) }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text(Strings.cancel) } },
    )
}

@Composable
private fun Section(title: String, content: @Composable ColumnScope.() -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.45f),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
            content()
        }
    }
}

@Composable
private fun Hint(text: String) {
    Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

const val APP_VERSION = "0.1.0"
