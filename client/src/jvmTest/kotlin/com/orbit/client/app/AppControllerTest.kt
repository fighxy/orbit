package com.orbit.client.app

import com.orbit.sdk.IdentityStatus
import com.orbit.sdk.OrbitEvent
import com.orbit.sdk.OrbitException
import com.orbit.sdk.bridge.OrbitErrorCode
import com.orbit.sdk.model.AccountId
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.DeviceId
import com.orbit.sdk.model.Message
import com.orbit.sdk.model.MessagePage
import com.orbit.sdk.model.Profile
import com.orbit.sdk.model.PublicIdentity
import com.orbit.sdk.model.Snapshot
import com.orbit.sdk.platform.SecureStorageException
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout

private class FakeBackend : ChatBackend {
    var profile: Profile? = null
    var closed = false
    override val events: Flow<OrbitEvent> = MutableSharedFlow()

    override suspend fun snapshot() = Snapshot(
        PublicIdentity(AccountId("01".repeat(32)), DeviceId("02".repeat(32)), "00".repeat(64)),
        profile,
        emptyList(),
    )

    override suspend fun messages(conversationId: ConversationId, beforeSeq: Long?, limit: Int) =
        MessagePage(emptyList(), hasMore = false)

    override suspend fun sendText(conversationId: ConversationId, text: String): Message = error("unused")

    override suspend fun updateProfile(displayName: String, about: String) =
        Profile(displayName.trim(), about.trim(), 1).also { profile = it }

    override suspend fun close() {
        closed = true
    }
}

private class FakeGateway(
    var status: IdentityStatus,
    private val passcode: String? = null,
    private val storageFailure: Boolean = false,
) : AccountGateway {
    val backends = mutableListOf<FakeBackend>()
    var createdWith: String? = "not created"
    var currentPasscode = passcode

    override suspend fun identityStatus(): IdentityStatus {
        if (storageFailure) throw SecureStorageException("no keyring")
        return status
    }

    override suspend fun createIdentity(passcode: String?) {
        createdWith = passcode
        currentPasscode = passcode
        status = if (passcode == null) IdentityStatus.Ready else IdentityStatus.PasscodeRequired
    }

    override suspend fun open(passcode: String?): ChatBackend {
        if (currentPasscode != null && passcode != currentPasscode) {
            throw OrbitException(OrbitErrorCode.WrongPasscode, "passcode is incorrect")
        }
        return FakeBackend().also { backends += it }
    }

    override suspend fun setPasscode(currentPasscode: String?, newPasscode: String) {
        if (this.currentPasscode != null && currentPasscode != this.currentPasscode) {
            throw OrbitException(OrbitErrorCode.WrongPasscode, "passcode is incorrect")
        }
        this.currentPasscode = newPasscode
    }

    override suspend fun removePasscode(currentPasscode: String) {
        if (currentPasscode != this.currentPasscode) throw OrbitException(OrbitErrorCode.WrongPasscode, "wrong")
        this.currentPasscode = null
    }
}

class AppControllerTest {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    private suspend inline fun <reified T : AppState> AppController.await(): T =
        withTimeout(5_000) { state.first { it is T } as T }

    @Test
    fun onboardingCreatesLockedAccountAndProfile() = runBlocking {
        val gateway = FakeGateway(IdentityStatus.Missing)
        val controller = AppController(gateway, scope)
        controller.start()
        assertEquals(OnboardingStep.Welcome, controller.await<AppState.Onboarding>().step)

        controller.beginOnboarding()
        controller.submitProfileDraft("   ", "")
        val invalid = controller.state.value as AppState.Onboarding
        assertEquals(OnboardingStep.Profile, invalid.step)
        assertTrue(invalid.error != null)

        controller.submitProfileDraft("Анна", "звёзды")
        assertEquals(OnboardingStep.Passcode, (controller.state.value as AppState.Onboarding).step)
        controller.completeOnboarding("12")
        assertTrue((controller.state.value as AppState.Onboarding).error != null)

        controller.completeOnboarding("1234")
        val ready = controller.await<AppState.Ready>()
        assertTrue(ready.passcodeEnabled)
        assertEquals("1234", gateway.createdWith)
        assertEquals("Анна", gateway.backends.single().profile?.displayName)
        scope.cancel()
    }

    @Test
    fun wrongPasscodeIsCountedThenUnlocks() = runBlocking {
        val controller = AppController(FakeGateway(IdentityStatus.PasscodeRequired, passcode = "secret"), scope)
        controller.start()
        controller.await<AppState.Locked>()

        controller.unlock("nope")
        val failed = withTimeout(5_000) { controller.state.first { it is AppState.Locked && it.failedAttempts == 1 } }
        assertIs<AppState.Locked>(failed)
        assertTrue(failed.error != null)

        controller.unlock("secret")
        assertTrue(controller.await<AppState.Ready>().passcodeEnabled)
        scope.cancel()
    }

    @Test
    fun lockNowClosesTheSessionOnlyWithPasscode() = runBlocking {
        val gateway = FakeGateway(IdentityStatus.Ready)
        val controller = AppController(gateway, scope)
        controller.start()
        controller.await<AppState.Ready>()
        controller.lockNow() // no passcode: ignored
        assertIs<AppState.Ready>(controller.state.value)

        assertNull(controller.setPasscode(null, "abcd"))
        assertTrue((controller.state.value as AppState.Ready).passcodeEnabled)
        controller.lockNow()
        controller.await<AppState.Locked>()
        assertTrue(gateway.backends.single().closed)

        controller.unlock("abcd")
        controller.await<AppState.Ready>()
        assertEquals(com.orbit.client.designsystem.Strings.wrongPasscode, controller.removePasscode("bad"))
        assertNull(controller.removePasscode("abcd"))
        assertEquals(false, (controller.state.value as AppState.Ready).passcodeEnabled)
        scope.cancel()
    }

    @Test
    fun unavailableSecureStorageIsReported() = runBlocking {
        val controller = AppController(FakeGateway(IdentityStatus.Ready, storageFailure = true), scope)
        controller.start()
        assertEquals("no keyring", controller.await<AppState.SecureStorageUnavailable>().details)
        scope.cancel()
    }

    @Test
    fun cooldownGrowsAfterFiveFailures() {
        assertEquals(0, AppController.cooldownFor(4))
        assertEquals(30, AppController.cooldownFor(5))
        assertEquals(60, AppController.cooldownFor(6))
        assertEquals(480, AppController.cooldownFor(9))
        assertEquals(480, AppController.cooldownFor(50))
    }
}
