package com.orbit.sdk

import com.orbit.sdk.bridge.IDENTITY_LOCKED_TAG
import com.orbit.sdk.bridge.JniNativeLibrary
import com.orbit.sdk.bridge.OrbitErrorCode
import com.orbit.sdk.bridge.SUPPORTED_ABI_VERSION
import com.orbit.sdk.model.MessageBody
import com.orbit.sdk.platform.DesktopSecretStore
import com.orbit.sdk.platform.SecretStore
import com.orbit.sdk.platform.SecureStorageException
import java.io.File
import java.util.UUID
import kotlin.io.path.createTempDirectory
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlinx.coroutines.runBlocking

private class MemorySecretStore : SecretStore {
    val values = HashMap<String, ByteArray>()

    override suspend fun read(key: String): ByteArray? = values[key]?.copyOf()

    override suspend fun write(key: String, value: ByteArray) {
        values[key] = value.copyOf()
    }

    override suspend fun delete(key: String) {
        values.remove(key)
    }
}

/** Runs the Kotlin SDK against the real Rust engine through JNI. */
class NativeIntegrationTest {
    private val library = JniNativeLibrary.load(
        System.getProperty("orbit.native.library") ?: error("orbit.native.library is not set"),
    )
    private val dataDir: File = createTempDirectory("orbit-test").toFile()

    @AfterTest
    fun cleanUp() {
        dataDir.deleteRecursively()
    }

    @Test
    fun abiVersionMatches() {
        assertEquals(SUPPORTED_ABI_VERSION, library.abiVersion)
    }

    @Test
    fun messagesSurviveRestart() = runBlocking {
        val sdk = OrbitSdk(library, MemorySecretStore(), dataDir.absolutePath)
        assertEquals(IdentityStatus.Missing, sdk.identityStatus())
        sdk.createIdentity()
        assertEquals(IdentityStatus.Ready, sdk.identityStatus())

        val first = sdk.open()
        val snapshot = first.snapshot()
        val conversation = snapshot.conversations.single().id
        val sent = first.sendText(conversation, "  hello from the JVM  ")
        assertEquals(MessageBody.Text("hello from the JVM"), sent.body)
        first.close()

        val second = sdk.open()
        assertEquals(snapshot.identity, second.snapshot().identity)
        val page = second.messages(conversation)
        assertEquals(listOf(sent), page.messages)
        assertFalse(page.hasMore)
        assertEquals(sent, second.snapshot().conversations.single().lastMessage)
        second.close()
    }

    @Test
    fun passcodeLocksTheStoredSecret() = runBlocking {
        val secrets = MemorySecretStore()
        val sdk = OrbitSdk(library, secrets, dataDir.absolutePath)
        sdk.createIdentity(passcode = "орбита-2026")
        assertEquals(IdentityStatus.PasscodeRequired, sdk.identityStatus())
        assertEquals(IDENTITY_LOCKED_TAG, secrets.values.getValue(OrbitSdk.DEFAULT_IDENTITY_KEY)[0])

        assertEquals(OrbitErrorCode.WrongPasscode, assertFailsWith<OrbitException> { sdk.open() }.code)
        assertEquals(OrbitErrorCode.WrongPasscode, assertFailsWith<OrbitException> { sdk.open("неверно") }.code)
        val client = sdk.open("орбита-2026")
        val identity = client.snapshot().identity
        client.close()

        // Changing the passcode keeps the same account.
        assertEquals(
            OrbitErrorCode.WrongPasscode,
            assertFailsWith<OrbitException> { sdk.setPasscode("неверно", "новый-код") }.code,
        )
        sdk.setPasscode("орбита-2026", "новый-код")
        assertEquals(OrbitErrorCode.WrongPasscode, assertFailsWith<OrbitException> { sdk.open("орбита-2026") }.code)
        sdk.open("новый-код").apply { assertEquals(identity, snapshot().identity) }.close()

        sdk.removePasscode("новый-код")
        assertEquals(IdentityStatus.Ready, sdk.identityStatus())
        sdk.open().apply { assertEquals(identity, snapshot().identity) }.close()
    }

    @Test
    fun shortPasscodeIsRejected() = runBlocking {
        val sdk = OrbitSdk(library, MemorySecretStore(), dataDir.absolutePath)
        sdk.createIdentity()
        assertEquals(OrbitErrorCode.InvalidArgument, assertFailsWith<OrbitException> { sdk.setPasscode(null, "123") }.code)
        assertEquals(IdentityStatus.Ready, sdk.identityStatus())
    }

    @Test
    fun profileIsStoredAndAnnounced() = runBlocking {
        val sdk = OrbitSdk(library, MemorySecretStore(), dataDir.absolutePath)
        sdk.createIdentity()
        val client = sdk.open()
        assertNull(client.snapshot().profile)
        val profile = client.updateProfile("  Анна ", "заметки о звёздах")
        assertEquals("Анна", profile.displayName)
        assertEquals(
            OrbitErrorCode.InvalidArgument,
            assertFailsWith<OrbitException> { client.updateProfile(" ", "") }.code,
        )
        client.close()
        sdk.open().apply { assertEquals(profile, snapshot().profile) }.close()
    }

    @Test
    fun secondEngineForSameAccountIsRejected() = runBlocking {
        val sdk = OrbitSdk(library, MemorySecretStore(), dataDir.absolutePath)
        sdk.createIdentity()
        val first = sdk.open()
        assertEquals(OrbitErrorCode.StorageLocked, assertFailsWith<OrbitException> { sdk.open() }.code)
        first.close()
        sdk.open().close()
    }

    @Test
    fun invalidInputIsReportedWithCodes() = runBlocking {
        val sdk = OrbitSdk(library, MemorySecretStore(), dataDir.absolutePath)
        sdk.createIdentity()
        val client = sdk.open()
        val conversation = client.snapshot().conversations.single().id
        assertEquals(
            OrbitErrorCode.InvalidArgument,
            assertFailsWith<OrbitException> { client.sendText(conversation, "   ") }.code,
        )
        assertEquals(
            OrbitErrorCode.InvalidArgument,
            assertFailsWith<OrbitException> { client.messages(conversation, limit = 0) }.code,
        )
        client.close()
    }

    @Test
    fun malformedSecretIsRejected() = runBlocking {
        val secrets = MemorySecretStore()
        secrets.write(OrbitSdk.DEFAULT_IDENTITY_KEY, byteArrayOf(1, 2, 3))
        val sdk = OrbitSdk(library, secrets, dataDir.absolutePath)
        assertEquals(OrbitErrorCode.InvalidIdentity, assertFailsWith<OrbitException> { sdk.open() }.code)
    }

    @Test
    fun secretAndPlaintextNeverReachTheDataDirectory() = runBlocking {
        val secrets = MemorySecretStore()
        val sdk = OrbitSdk(library, secrets, dataDir.absolutePath)
        sdk.createIdentity()
        val client = sdk.open()
        val conversation = client.snapshot().conversations.single().id
        val marker = "plaintext-marker-${UUID.randomUUID()}"
        client.sendText(conversation, marker)
        client.close()

        val secret = secrets.values.getValue(OrbitSdk.DEFAULT_IDENTITY_KEY)
        dataDir.walkTopDown().filter { it.isFile }.forEach { file ->
            val bytes = file.readBytes()
            assertFalse(bytes.containsSlice(marker.encodeToByteArray()), "plaintext found in ${file.name}")
            assertFalse(bytes.containsSlice(secret.copyOfRange(1, 33)), "account seed found in ${file.name}")
        }
    }

    @Test
    fun desktopKeyringRoundTripsOrFailsClosed() = runBlocking {
        val store = DesktopSecretStore(library, service = "com.orbit.messenger.test")
        val key = "test/${UUID.randomUUID()}"
        val value = byteArrayOf(1, 2, 3, 4)
        try {
            store.write(key, value)
        } catch (e: SecureStorageException) {
            // No keyring on this machine (for example headless CI): nothing may be stored elsewhere.
            println("desktop keyring unavailable: ${e.message}")
            return@runBlocking
        }
        try {
            assertContentEquals(value, store.read(key))
        } finally {
            store.delete(key)
        }
        assertNull(store.read(key))
    }
}

private fun ByteArray.containsSlice(needle: ByteArray): Boolean =
    (0..size - needle.size).any { start -> needle.indices.all { this[start + it] == needle[it] } }
