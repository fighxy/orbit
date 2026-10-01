package com.orbit.client.features.onboarding

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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.orbit.client.app.AppController
import com.orbit.client.app.AppState
import com.orbit.client.app.OnboardingStep
import com.orbit.client.app.PasscodeRules
import com.orbit.client.app.ProfileRules
import com.orbit.client.designsystem.Avatar
import com.orbit.client.designsystem.OrbitMark
import com.orbit.client.designsystem.PasscodeField
import com.orbit.client.designsystem.Strings

@Composable
fun OnboardingScreen(state: AppState.Onboarding, controller: AppController) {
    Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState()), contentAlignment = Alignment.Center) {
        Column(
            modifier = Modifier.widthIn(max = 460.dp).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(20.dp),
        ) {
            when (state.step) {
                OnboardingStep.Welcome -> WelcomeStep(onStart = controller::beginOnboarding)
                OnboardingStep.Profile -> ProfileStep(state, controller)
                OnboardingStep.Passcode -> PasscodeStep(state, controller)
            }
            state.error?.let { Text(it, color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center) }
        }
    }
}

@Composable
private fun WelcomeStep(onStart: () -> Unit) {
    OrbitMark(size = 72.dp)
    Text(Strings.onboardingTitle, style = MaterialTheme.typography.headlineMedium, textAlign = TextAlign.Center)
    Text(Strings.onboardingBody, style = MaterialTheme.typography.bodyLarge, textAlign = TextAlign.Center)
    Row(verticalAlignment = Alignment.Top) {
        Icon(
            Icons.Filled.Lock,
            contentDescription = null,
            modifier = Modifier.size(18.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.width(8.dp))
        Text(
            Strings.onboardingLimits,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    Button(onClick = onStart) { Text(Strings.start) }
}

@Composable
private fun ProfileStep(state: AppState.Onboarding, controller: AppController) {
    var name by rememberSaveable { mutableStateOf(state.displayName) }
    var about by rememberSaveable { mutableStateOf(state.about) }
    val submit = { controller.submitProfileDraft(name, about) }

    Avatar(name.ifBlank { null }, size = 72.dp)
    Text(Strings.profileStepTitle, style = MaterialTheme.typography.headlineSmall, textAlign = TextAlign.Center)
    Text(
        Strings.profileStepBody,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.Center,
    )
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
        placeholder = { Text(Strings.aboutHint) },
        supportingText = { Text("${about.length} / ${ProfileRules.MAX_ABOUT}") },
        maxLines = 3,
        modifier = Modifier.fillMaxWidth(),
    )
    StepButtons(
        primary = Strings.next,
        primaryEnabled = name.isNotBlank(),
        onPrimary = submit,
        onBack = controller::onboardingBack,
    )
}

@Composable
private fun PasscodeStep(state: AppState.Onboarding, controller: AppController) {
    var passcode by rememberSaveable { mutableStateOf("") }
    var confirmation by rememberSaveable { mutableStateOf("") }
    val mismatch = confirmation.isNotEmpty() && confirmation != passcode
    val valid = PasscodeRules.validate(passcode) == null && passcode == confirmation

    Icon(Icons.Filled.Lock, contentDescription = null, modifier = Modifier.size(48.dp), tint = MaterialTheme.colorScheme.primary)
    Text(Strings.passcodeStepTitle, style = MaterialTheme.typography.headlineSmall)
    Text(Strings.passcodeStepBody, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center)
    Text(
        Strings.passcodeWarning,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.Center,
    )
    PasscodeField(
        value = passcode,
        onValueChange = { passcode = it },
        label = Strings.newPasscode,
        enabled = !state.busy,
        imeAction = ImeAction.Next,
        modifier = Modifier.fillMaxWidth(),
    )
    PasscodeField(
        value = confirmation,
        onValueChange = { confirmation = it },
        label = Strings.confirmPasscode,
        isError = mismatch,
        enabled = !state.busy,
        onImeAction = { if (valid) controller.completeOnboarding(passcode) },
        modifier = Modifier.fillMaxWidth(),
    )
    if (mismatch) Text(Strings.passcodeMismatch, color = MaterialTheme.colorScheme.error)
    if (state.busy) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            Spacer(Modifier.width(12.dp))
            Text(Strings.creatingAccount)
        }
    } else {
        Button(onClick = { controller.completeOnboarding(passcode) }, enabled = valid) {
            Text(Strings.setPasscodeAndCreate)
        }
        Row {
            TextButton(onClick = controller::onboardingBack) { Text(Strings.back) }
            TextButton(onClick = { controller.completeOnboarding(null) }) { Text(Strings.skip) }
        }
    }
}

@Composable
private fun StepButtons(primary: String, primaryEnabled: Boolean, onPrimary: () -> Unit, onBack: () -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
        TextButton(onClick = onBack) { Text(Strings.back) }
        Button(onClick = onPrimary, enabled = primaryEnabled) { Text(primary) }
    }
}
