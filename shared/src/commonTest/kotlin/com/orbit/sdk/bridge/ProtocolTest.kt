package com.orbit.sdk.bridge

import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.ConversationKind
import com.orbit.sdk.model.MessageBody
import com.orbit.sdk.model.MessageState
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull

/** Golden JSON emitted by orbit-core; keeps the Kotlin mirror of the protocol honest. */
class ProtocolTest {
    @Test
    fun networkCommandsAndNotificationsMatchCoreContract() {
        assertEquals("""{"type":"register_node","node":"node","registration_code":"code"}""",
            WireCommand.RegisterNode("node", "code").toJsonBytes().decodeToString())
        assertEquals("""{"type":"accept_invite","text":"orbit://invite/test"}""",
            WireCommand.AcceptInvite("orbit://invite/test").toJsonBytes().decodeToString())
        val batch = decodeBatch("""{"events":[
            {"seq":1,"event":{"type":"network_changed","network":{"node":"n","state":"online","error":null}}},
            {"seq":2,"event":{"type":"contacts_changed"}},
            {"seq":3,"event":{"type":"command_succeeded","request_id":1,"result":{"type":"invite_inspected","preview":{"account_id":"${"01".repeat(32)}","device_id":"${"02".repeat(32)}","display_name":"Алиса","expires_at_ms":50}}}}
        ]}""".encodeToByteArray())
        assertEquals(com.orbit.sdk.model.ConnectionState.Online, assertIs<WireEvent.NetworkChanged>(batch.events[0].event).network.state)
        assertEquals(WireEvent.ContactsChanged, batch.events[1].event)
        val result = assertIs<WireResult.InviteInspected>(assertIs<WireEvent.CommandSucceeded>(batch.events[2].event).result)
        assertEquals("Алиса", result.preview.displayName)
        assertEquals(OrbitErrorCode.InvalidInvite, OrbitErrorCode.of("invalid_invite"))
        assertEquals(18, OrbitErrorCode.Network.value)
    }
    @Test
    fun commandsEncodeLikeRust() {
        val id = ConversationId("11".repeat(16))
        assertEquals("""{"type":"get_snapshot"}""", WireCommand.GetSnapshot.toJsonBytes().decodeToString())
        assertEquals(
            """{"type":"list_messages","conversation_id":"${id.hex}","limit":20}""",
            WireCommand.ListMessages(id, limit = 20).toJsonBytes().decodeToString(),
        )
        assertEquals(
            """{"type":"list_messages","conversation_id":"${id.hex}","before_seq":7,"limit":20}""",
            WireCommand.ListMessages(id, beforeSeq = 7, limit = 20).toJsonBytes().decodeToString(),
        )
        assertEquals(
            """{"type":"send_text","conversation_id":"${id.hex}","text":"привет"}""",
            WireCommand.SendText(id, "привет").toJsonBytes().decodeToString(),
        )
    }

    @Test
    fun decodesSnapshotResult() {
        val json = """{"events":[{"seq":1,"event":{"type":"command_succeeded","request_id":1,"result":{"type":"snapshot","identity":{"account_id":"f596e6cbe905c6f19b04ae48e7be8fc71927465c9539f15109c8c3898ad9b4aa","device_id":"8562df8cbb47452adf38dab2af212b283ceeca9026c44af7eb1b45428ddd2698","device_certificate":"76713b4b0b78d8c9cd8eec1a162d01132b45f3802bc0d767da16c3b136923886046ba23ac579a28fdfca86d19de39b681839015526fe57b16da89b2d55715800"},"conversations":[{"id":"bb347f31ab23288f56a2526d4cd1d2bc","kind":"saved_messages","created_at_ms":1790835410764,"last_message":null}]}}}]}"""
        val event = decodeBatch(json.encodeToByteArray()).events.single()
        assertEquals(1, event.seq)
        val succeeded = assertIs<WireEvent.CommandSucceeded>(event.event)
        val snapshot = assertIs<WireResult.Snapshot>(succeeded.result)
        assertEquals("F596 E6CB E905 C6F1", snapshot.identity.accountFingerprint)
        val conversation = snapshot.conversations.single()
        assertEquals(ConversationKind.SavedMessages, conversation.kind)
        assertNull(conversation.lastMessage)
    }

    @Test
    fun decodesMessageEventsAndFailures() {
        val message = """{"id":"${"ab".repeat(16)}","conversation_id":"${"cd".repeat(16)}","seq":3,"author_account":"${"01".repeat(32)}","author_device":"${"02".repeat(32)}","created_at_ms":5,"body":{"type":"text","text":"hi"},"state":"saved_locally"}"""
        val json = """{"events":[
            {"seq":2,"event":{"type":"message_added","message":$message}},
            {"seq":3,"event":{"type":"command_failed","request_id":9,"error":{"code":"invalid_argument","message":"message text is empty"}}},
            {"seq":5,"event":{"type":"resync_required"}}
        ]}"""
        val events = decodeBatch(json.encodeToByteArray()).events.map { it.event }

        val added = assertIs<WireEvent.MessageAdded>(events[0])
        assertEquals(MessageBody.Text("hi"), added.message.body)
        assertEquals(MessageState.SavedLocally, added.message.state)
        assertEquals(3, added.message.seq)

        val failed = assertIs<WireEvent.CommandFailed>(events[1])
        assertEquals(OrbitErrorCode.InvalidArgument, OrbitErrorCode.of(failed.error.code))
        assertEquals(WireEvent.ResyncRequired, events[2])
    }

    @Test
    fun errorCodesMatchAbi() {
        assertEquals(12, OrbitErrorCode.Closed.value)
        assertEquals("storage_key_mismatch", OrbitErrorCode.StorageKeyMismatch.wireName)
        assertEquals(OrbitErrorCode.UnsupportedStorageVersion, OrbitErrorCode.of("unsupported_storage_version"))
        assertEquals(OrbitErrorCode.Unknown, OrbitErrorCode.of(999))
    }
}
