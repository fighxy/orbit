package com.orbit.client.features.lock

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.orbit.client.app.AppState
import com.orbit.client.designsystem.OrbitMark
import com.orbit.client.designsystem.PasscodeField
import com.orbit.client.designsystem.Strings

@Composable
fun LockScreen(state: AppState.Locked, onUnlock: (String) -> Unit) {
    // Not saveable on purpose: the passcode must not outlive this screen.
    var passcode by remember { mutableStateOf("") }
    val focus = remember { FocusRequester() }
    val canSubmit = passcode.isNotEmpty() && !state.unlocking && state.cooldownSeconds == 0
    val submit = {
        if (canSubmit) {
            onUnlock(passcode)
            passcode = ""
        }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }

    Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState()), contentAlignment = Alignment.Center) {
        Column(
            modifier = Modifier.widthIn(max = 380.dp).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            OrbitMark(size = 64.dp)
            Text(Strings.lockTitle, style = MaterialTheme.typography.headlineSmall)
            Text(Strings.lockBody, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center)
            PasscodeField(
                value = passcode,
                onValueChange = { passcode = it },
                label = Strings.passcode,
                isError = state.error != null,
                enabled = !state.unlocking,
                onImeAction = submit,
                modifier = Modifier.fillMaxWidth().focusRequester(focus),
            )
            when {
                state.cooldownSeconds > 0 -> Text(
                    Strings.cooldown(state.cooldownSeconds),
                    color = MaterialTheme.colorScheme.error,
                    textAlign = TextAlign.Center,
                )
                state.error != null -> Text(state.error, color = MaterialTheme.colorScheme.error)
            }
            if (state.unlocking) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(12.dp))
                    Text(Strings.unlocking)
                }
            } else {
                Button(onClick = submit, enabled = canSubmit, modifier = Modifier.fillMaxWidth()) {
                    Text(Strings.unlock)
                }
            }
            if (state.failedAttempts > 0) {
                Text(
                    Strings.failedAttempts(state.failedAttempts),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                Strings.forgotPasscode,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        }
    }
}
