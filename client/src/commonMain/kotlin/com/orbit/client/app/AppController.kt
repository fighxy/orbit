package com.orbit.client.app

import com.orbit.client.designsystem.Strings
import com.orbit.client.features.chat.ChatSession
import com.orbit.sdk.OrbitException
import com.orbit.sdk.platform.SecureStorageException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

sealed interface AppState {
    data object Starting : AppState

    data class NeedsIdentity(val creating: Boolean = false) : AppState

    /** The OS secure store is unavailable; nothing was stored elsewhere. */
    data class SecureStorageUnavailable(val details: String) : AppState

    data class Failed(val title: String, val details: String) : AppState

    data class Ready(val session: ChatSession) : AppState
}

/** Top-level state machine: identity check, onboarding, opening the engine. */
class AppController(
    private val gateway: AccountGateway,
    private val scope: CoroutineScope,
) {
    private val mutableState = MutableStateFlow<AppState>(AppState.Starting)
    val state: StateFlow<AppState> = mutableState.asStateFlow()

    fun start() {
        mutableState.value = AppState.Starting
        scope.launch {
            mutableState.value = guarded {
                if (gateway.identityStatus() == com.orbit.sdk.IdentityStatus.Missing) AppState.NeedsIdentity() else openSession()
            }
        }
    }

    fun createIdentity() {
        val current = mutableState.value
        if (current !is AppState.NeedsIdentity || current.creating) return
        mutableState.value = AppState.NeedsIdentity(creating = true)
        scope.launch {
            mutableState.value = guarded {
                gateway.createIdentity(null)
                openSession()
            }
        }
    }

    /** Closes the engine; call before the process or window goes away. */
    suspend fun close() {
        (mutableState.value as? AppState.Ready)?.session?.close()
        mutableState.value = AppState.Starting
    }

    private suspend fun openSession(): AppState =
        AppState.Ready(ChatSession(gateway.open(null), scope).also { it.start() })

    private suspend fun guarded(block: suspend () -> AppState): AppState = try {
        block()
    } catch (e: SecureStorageException) {
        AppState.SecureStorageUnavailable(e.message ?: "")
    } catch (e: OrbitException) {
        AppState.Failed(Strings.openFailedTitle, Strings.describe(e))
    }
}
