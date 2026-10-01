package com.orbit.sdk

import com.orbit.sdk.bridge.NativeEngine
import com.orbit.sdk.bridge.OrbitErrorCode
import com.orbit.sdk.bridge.OrbitNativeException
import com.orbit.sdk.model.ConversationId
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout

/**
 * Blocking fake engine: answers synchronously inside [submit], before the
 * request ID is returned, which is the worst case for result correlation.
 */
private class FakeEngine(private val respond: (requestId: Long, command: String) -> List<String>) : NativeEngine {
    private val queue = LinkedBlockingQueue<String>()
    private val nextRequest = AtomicLong(1)
    private val nextSeq = AtomicLong(1)

    @Volatile
    private var closed = false

    override fun submit(commandJson: ByteArray): Long {
        if (closed) throw OrbitNativeException(OrbitErrorCode.Closed.value, "engine is closed")
        val id = nextRequest.getAndIncrement()
        respond(id, commandJson.decodeToString()).forEach(queue::put)
        return id
    }

    override fun waitEvents(timeoutMs: Int): ByteArray {
        if (closed) throw OrbitNativeException(OrbitErrorCode.Closed.value, "engine is closed")
        val first = queue.poll(timeoutMs.toLong(), TimeUnit.MILLISECONDS)
        if (closed) throw OrbitNativeException(OrbitErrorCode.Closed.value, "engine is closed")
        val events = buildList {
            if (first != null && first != WAKE) add(first)
            while (true) add(queue.poll()?.takeIf { it != WAKE } ?: break)
        }
        val body = events.joinToString(",") { """{"seq":${nextSeq.getAndIncrement()},"event":$it}""" }
        return """{"events":[$body]}""".encodeToByteArray()
    }

    override fun cancelWait() = queue.put(WAKE)

    override fun close() {
        closed = true
        queue.put(WAKE)
    }

    companion object {
        const val WAKE = "wake"
    }
}

private val conversation = ConversationId("cd".repeat(16))

private fun message(text: String, seq: Int) =
    """{"id":"${"%032x".format(seq)}","conversation_id":"${conversation.hex}","seq":$seq,"author_account":"${"01".repeat(32)}","author_device":"${"02".repeat(32)}","created_at_ms":1,"body":{"type":"text","text":"$text"},"state":"saved_locally"}"""

private fun succeeded(id: Long, result: String) = """{"type":"command_succeeded","request_id":$id,"result":$result}"""

class OrbitClientTest {
    @Test
    fun correlatesResultsThatArriveBeforeSubmitReturns() = runBlocking {
        val seq = AtomicLong(0)
        val client = OrbitClient(
            FakeEngine { id, _ ->
                listOf(succeeded(id, """{"type":"message_saved","message":${message("m$id", seq.incrementAndGet().toInt())}}"""))
            },
            Dispatchers.IO,
        )
        val sent = (1..200).map { i -> async(Dispatchers.Default) { client.sendText(conversation, "m$i") } }.awaitAll()
        assertEquals(200, sent.map { it.id }.toSet().size)
        client.close()
    }

    @Test
    fun commandFailureCarriesErrorCode() = runBlocking {
        val client = OrbitClient(
            FakeEngine { id, _ ->
                listOf("""{"type":"command_failed","request_id":$id,"error":{"code":"invalid_argument","message":"message text is empty"}}""")
            },
            Dispatchers.IO,
        )
        val error = assertFailsWith<OrbitException> { client.sendText(conversation, " ") }
        assertEquals(OrbitErrorCode.InvalidArgument, error.code)
        assertEquals("message text is empty", error.message)
        client.close()
    }

    @Test
    fun notificationsAreDeliveredToSubscribers() = runBlocking {
        val client = OrbitClient(
            FakeEngine { id, _ ->
                val msg = message("hello", 1)
                listOf("""{"type":"message_added","message":$msg}""", succeeded(id, """{"type":"message_saved","message":$msg}"""))
            },
            Dispatchers.IO,
        )
        val received = async { client.events.first() }
        delay(50) // let the subscriber attach before the event is emitted
        client.sendText(conversation, "hello")
        val event = assertIs<OrbitEvent.MessageAdded>(withTimeout(5_000) { received.await() })
        assertEquals("hello", (event.message.body as com.orbit.sdk.model.MessageBody.Text).text)
        client.close()
    }

    @Test
    fun closeFailsPendingCallsAndLaterCalls() = runBlocking {
        val client = OrbitClient(FakeEngine { _, _ -> emptyList() }, Dispatchers.IO)
        val pending = async { runCatching { client.snapshot() } }
        delay(100)
        client.close()
        val error = assertIs<OrbitException>(pending.await().exceptionOrNull())
        assertEquals(OrbitErrorCode.Closed, error.code)
        assertEquals(ClientState.Closed, client.state.value)
        assertEquals(OrbitErrorCode.Closed, assertFailsWith<OrbitException> { client.snapshot() }.code)
        client.close()
    }

    @Test
    fun malformedEventsFailTheClientVisibly() = runBlocking {
        val client = OrbitClient(FakeEngine { _, _ -> listOf("""{"type":"unknown_event"}""") }, Dispatchers.IO)
        val error = assertFailsWith<OrbitException> { client.snapshot() }
        assertEquals(OrbitErrorCode.Internal, error.code)
        assertIs<ClientState.Failed>(client.state.value)
        client.close()
    }
}
