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
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import com.orbit.client.designsystem.OrbitTheme
import com.orbit.client.designsystem.Strings
import com.orbit.client.features.chat.MessengerScreen
import com.orbit.client.features.chat.VoiceHost
import com.orbit.client.features.host.NodeHost
import com.orbit.client.features.settings.AvatarPick
import com.orbit.client.features.lock.LockScreen
import com.orbit.client.features.settings.SecurityActions
import com.orbit.client.features.onboarding.OnboardingScreen
import com.orbit.client.features.status.LoadingScreen
import com.orbit.client.features.status.StatusScreen

/** Compose root shared by Android, iOS and the JVM desktop app. */
@Composable
fun OrbitApp(
    controller: AppController,
    preferences: PreferencesRepository,
    navigation: ShellNavigation = remember { ShellNavigation() },
    linuxDesktop: Boolean = false,
    prepareLocalNetwork: (suspend () -> Boolean)? = null,
    localNetworkHint: String = Strings.localNetworkHint,
    nodeHost: NodeHost? = null,
    voice: VoiceHost? = null,
    pickAvatar: (suspend () -> AvatarPick)? = null,
) {
    val prefs by preferences.state.collectAsState()
    OrbitTheme(prefs.theme) {
        Surface(modifier = Modifier.fillMaxSize()) {
            val state by controller.state.collectAsState()
            // Keeps content clear of system bars, cutouts and the keyboard.
            Box(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
                when (val current = state) {
                    AppState.Starting -> LoadingScreen()
                    is AppState.Onboarding -> OnboardingScreen(current, controller)
                    is AppState.Locked -> LockScreen(current, onUnlock = controller::unlock)
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
                    is AppState.Ready -> MessengerScreen(
                        session = current.session,
                        preferences = preferences,
                        navigation = navigation,
                        prepareLocalNetwork = prepareLocalNetwork,
                        localNetworkHint = localNetworkHint,
                        nodeHost = nodeHost,
                        voice = voice,
                        pickAvatar = pickAvatar,
                        security = object : SecurityActions {
                            override val passcodeEnabled = current.passcodeEnabled

                            override suspend fun setPasscode(currentPasscode: String?, newPasscode: String) =
                                controller.setPasscode(currentPasscode, newPasscode)

                            override suspend fun removePasscode(currentPasscode: String) =
                                controller.removePasscode(currentPasscode)

                            override fun lockNow() = controller.lockNow()
                        },
                    )
                }
            }
        }
    }
}
