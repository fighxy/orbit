package com.orbit.client.app

import com.orbit.client.designsystem.Strings
import com.orbit.client.features.chat.ChatSession
import com.orbit.sdk.IdentityStatus
import com.orbit.sdk.OrbitException
import com.orbit.sdk.bridge.OrbitErrorCode
import com.orbit.sdk.platform.SecureStorageException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

enum class OnboardingStep { Welcome, Profile, Passcode }

sealed interface AppState {
    data object Starting : AppState

    /** Account creation. [displayName] and [about] carry the draft between steps. */
    data class Onboarding(
        val step: OnboardingStep = OnboardingStep.Welcome,
        val displayName: String = "",
        val about: String = "",
        val busy: Boolean = false,
        val error: String? = null,
    ) : AppState

    /** The stored secret is sealed under a passcode. */
    data class Locked(
        val unlocking: Boolean = false,
        val error: String? = null,
        val failedAttempts: Int = 0,
        /** Seconds before the next attempt is accepted; 0 when none. */
        val cooldownSeconds: Int = 0,
    ) : AppState

    /** The OS secure store is unavailable; nothing was stored elsewhere. */
    data class SecureStorageUnavailable(val details: String) : AppState

    data class Failed(val title: String, val details: String) : AppState

    data class Ready(val session: ChatSession, val passcodeEnabled: Boolean) : AppState
}

/** Top-level state machine: onboarding, unlock, opening and locking the account. */
class AppController(
    private val gateway: AccountGateway,
    private val scope: CoroutineScope,
) {
    private val mutableState = MutableStateFlow<AppState>(AppState.Starting)
    val state: StateFlow<AppState> = mutableState.asStateFlow()

    private var cooldownJob: Job? = null

    fun start() {
        mutableState.value = AppState.Starting
        scope.launch {
            mutableState.value = guarded {
                when (gateway.identityStatus()) {
                    IdentityStatus.Missing -> AppState.Onboarding()
                    IdentityStatus.PasscodeRequired -> AppState.Locked()
                    IdentityStatus.Ready -> openSession(passcode = null)
                }
            }
        }
    }

    // Onboarding

    fun beginOnboarding() = updateOnboarding { it.copy(step = OnboardingStep.Profile, error = null) }

    fun onboardingBack() = updateOnboarding {
        when (it.step) {
            OnboardingStep.Welcome, OnboardingStep.Profile -> it.copy(step = OnboardingStep.Welcome, error = null)
            OnboardingStep.Passcode -> it.copy(step = OnboardingStep.Profile, error = null)
        }
    }

    fun submitProfileDraft(displayName: String, about: String) = updateOnboarding {
        val error = ProfileRules.validate(displayName, about)
        if (error != null) {
            it.copy(displayName = displayName, about = about, error = error)
        } else {
            it.copy(step = OnboardingStep.Passcode, displayName = displayName, about = about, error = null)
        }
    }

    /** Creates the account; [passcode] is optional. */
    fun completeOnboarding(passcode: String?) {
        val draft = mutableState.value as? AppState.Onboarding ?: return
        if (draft.busy) return
        if (passcode != null) PasscodeRules.validate(passcode)?.let { error ->
            mutableState.value = draft.copy(error = error)
            return
        }
        mutableState.value = draft.copy(busy = true, error = null)
        scope.launch {
            mutableState.value = guarded {
                gateway.createIdentity(passcode)
                val ready = openSession(passcode)
                ready.session.updateProfile(draft.displayName, draft.about)?.let { error ->
                    // The account exists; the profile can still be set in settings.
                    ready.session.showBanner(error)
                }
                ready
            }
        }
    }

    // Unlock and lock

    fun unlock(passcode: String) {
        val locked = mutableState.value as? AppState.Locked ?: return
        if (locked.unlocking || locked.cooldownSeconds > 0) return
        if (passcode.isEmpty()) return
        mutableState.value = locked.copy(unlocking = true, error = null)
        scope.launch {
            try {
                mutableState.value = openSession(passcode)
            } catch (e: OrbitException) {
                if (e.code != OrbitErrorCode.WrongPasscode) {
                    mutableState.value = AppState.Failed(Strings.openFailedTitle, Strings.describe(e))
                    return@launch
                }
                val failures = locked.failedAttempts + 1
                mutableState.value = AppState.Locked(error = Strings.wrongPasscode, failedAttempts = failures)
                startCooldown(cooldownFor(failures))
            } catch (e: SecureStorageException) {
                mutableState.value = AppState.SecureStorageUnavailable(e.message ?: "")
            }
        }
    }

    /** Closes the account and returns to the lock screen. Requires a passcode. */
    fun lockNow() {
        val ready = mutableState.value as? AppState.Ready ?: return
        if (!ready.passcodeEnabled) return
        mutableState.value = AppState.Starting
        scope.launch {
            ready.session.close()
            mutableState.value = AppState.Locked()
        }
    }

    // Passcode management (settings). Return an error message or null.

    suspend fun setPasscode(currentPasscode: String?, newPasscode: String): String? {
        val ready = mutableState.value as? AppState.Ready ?: return Strings.notReady
        PasscodeRules.validate(newPasscode)?.let { return it }
        return passcodeOperation { gateway.setPasscode(currentPasscode.takeIf { ready.passcodeEnabled }, newPasscode) }
            ?: run {
                mutableState.update { if (it is AppState.Ready) it.copy(passcodeEnabled = true) else it }
                null
            }
    }

    suspend fun removePasscode(currentPasscode: String): String? {
        if (mutableState.value !is AppState.Ready) return Strings.notReady
        return passcodeOperation { gateway.removePasscode(currentPasscode) }
            ?: run {
                mutableState.update { if (it is AppState.Ready) it.copy(passcodeEnabled = false) else it }
                null
            }
    }

    /** Closes the engine; call before the process or window goes away. */
    suspend fun close() {
        cooldownJob?.cancel()
        (mutableState.value as? AppState.Ready)?.session?.close()
        mutableState.value = AppState.Starting
    }

    private suspend fun openSession(passcode: String?): AppState.Ready {
        val session = ChatSession(gateway.open(passcode), scope)
        session.start()
        return AppState.Ready(session, passcodeEnabled = passcode != null)
    }

    private suspend fun passcodeOperation(block: suspend () -> Unit): String? = try {
        block()
        null
    } catch (e: OrbitException) {
        if (e.code == OrbitErrorCode.WrongPasscode) Strings.wrongPasscode else Strings.describe(e)
    } catch (e: SecureStorageException) {
        Strings.secureStorageTitle + ": " + (e.message ?: "")
    }

    private fun startCooldown(seconds: Int) {
        cooldownJob?.cancel()
        if (seconds <= 0) return
        cooldownJob = scope.launch {
            for (left in seconds downTo 1) {
                mutableState.update { if (it is AppState.Locked) it.copy(cooldownSeconds = left) else it }
                delay(1_000)
            }
            mutableState.update { if (it is AppState.Locked) it.copy(cooldownSeconds = 0) else it }
        }
    }

    private inline fun updateOnboarding(change: (AppState.Onboarding) -> AppState.Onboarding) {
        mutableState.update { if (it is AppState.Onboarding && !it.busy) change(it) else it }
    }

    private suspend fun guarded(block: suspend () -> AppState): AppState = try {
        block()
    } catch (e: SecureStorageException) {
        AppState.SecureStorageUnavailable(e.message ?: "")
    } catch (e: OrbitException) {
        AppState.Failed(Strings.openFailedTitle, Strings.describe(e))
    }

    companion object {
        /**
         * UI throttle after repeated failures: none for the first 4 attempts,
         * then 30 s doubling up to 8 min. It lives in memory only; the
         * Argon2 cost is what slows down offline guessing.
         */
        fun cooldownFor(failedAttempts: Int): Int =
            if (failedAttempts < 5) 0 else 30 shl minOf(failedAttempts - 5, 4)
    }
}

object PasscodeRules {
    const val MIN_LENGTH = 4

    fun validate(passcode: String): String? =
        if (passcode.length < MIN_LENGTH) Strings.passcodeTooShort else null
}

object ProfileRules {
    const val MAX_NAME = 64
    const val MAX_ABOUT = 140

    fun validate(displayName: String, about: String): String? = when {
        displayName.isBlank() -> Strings.nameRequired
        displayName.trim().length > MAX_NAME -> Strings.nameTooLong
        about.trim().length > MAX_ABOUT -> Strings.aboutTooLong
        else -> null
    }
}
