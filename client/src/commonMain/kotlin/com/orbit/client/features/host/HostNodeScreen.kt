package com.orbit.client.features.host

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import com.orbit.client.designsystem.Strings
import kotlinx.coroutines.launch

/** Windows entry for hosting a node. Address and code appear after a successful start. */
@Composable
fun HostNodeScreen(host: NodeHost, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var address by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    var busy by rememberSaveable { mutableStateOf(false) }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    val started = address.isNotEmpty() && code.isNotEmpty()

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            TextButton(onClick = onBack, enabled = !busy) { Text(Strings.back) }
            Text(Strings.becomeNode, style = MaterialTheme.typography.headlineSmall)
        }
        HorizontalDivider()
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(20.dp).widthIn(max = 680.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(Strings.becomeNodeHint)
            Button(
                onClick = {
                    busy = true
                    error = null
                    scope.launch {
                        try {
                            val card = host.becomeNode()
                            address = card.address
                            code = card.registrationCode
                        } catch (exception: Exception) {
                            error = exception.message ?: Strings.becomeNodeFailed
                        } finally {
                            busy = false
                        }
                    }
                },
                enabled = !busy,
            ) {
                Text(Strings.becomeNode)
            }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (started) {
                Text(Strings.becomeNodeRunning, color = MaterialTheme.colorScheme.primary)
                OutlinedTextField(
                    value = address,
                    onValueChange = {},
                    readOnly = true,
                    label = { Text(Strings.nodeAddress) },
                    textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                    modifier = Modifier.fillMaxWidth(),
                    maxLines = 4,
                )
                OutlinedTextField(
                    value = code,
                    onValueChange = {},
                    readOnly = true,
                    label = { Text(Strings.hostedRegistrationCode) },
                    textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                Text(Strings.becomeNodeShareHint, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}
