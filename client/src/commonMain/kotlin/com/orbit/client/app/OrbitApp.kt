package com.orbit.client.app

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import com.orbit.client.designsystem.OrbitTheme
import com.orbit.client.designsystem.Strings
import com.orbit.client.features.chat.MessengerScreen
import com.orbit.client.features.onboarding.OnboardingScreen
import com.orbit.client.features.status.LoadingScreen
import com.orbit.client.features.status.StatusScreen

/** Compose root shared by Android, iOS and the JVM desktop app. */
@Composable
fun OrbitApp(controller: AppController, linuxDesktop: Boolean = false) {
    OrbitTheme {
        Surface(modifier = Modifier.fillMaxSize()) {
            val state by controller.state.collectAsState()
            // Keeps content clear of system bars, cutouts and the keyboard.
            Box(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
                when (val current = state) {
                    AppState.Starting -> LoadingScreen()
                    is AppState.NeedsIdentity -> OnboardingScreen(
                        creating = current.creating,
                        onCreate = controller::createIdentity,
                    )
                    is AppState.SecureStorageUnavailable -> StatusScreen(
                        title = Strings.secureStorageTitle,
                        body = Strings.secureStorageBody,
                        hint = if (linuxDesktop) Strings.secureStorageLinuxHint else null,
                        details = current.details,
                        onRetry = controller::start,
                    )
                    is AppState.Failed -> StatusScreen(
                        title = current.title,
                        body = current.details,
                        onRetry = controller::start,
                    )
                    is AppState.Ready -> MessengerScreen(current.session)
                }
            }
        }
    }
}
