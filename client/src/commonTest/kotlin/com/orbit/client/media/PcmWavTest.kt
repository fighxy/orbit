package com.orbit.client.media

import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNull

class PcmWavTest {
    @Test
    fun hundredMillisecondsRoundTrips() {
        val pcm = ByteArray(3_200) { index -> (index * 3).toByte() }
        val wav = VoicePcm.pcm16leToWav(pcm)
        assertEquals(3_244, wav.size)
        assertEquals("RIFF", wav.decodeToString(0, 4))
        assertEquals("WAVE", wav.decodeToString(8, 12))
        assertEquals("fmt ", wav.decodeToString(12, 16))
        assertEquals("data", wav.decodeToString(36, 40))
        assertEquals(16_000, readU32(wav, 24))
        assertEquals(1, readU16(wav, 20))
        assertEquals(1, readU16(wav, 22))
        assertEquals(16, readU16(wav, 34))
        assertEquals(3_200, readU32(wav, 40))
        assertContentEquals(pcm, assertNotNull(VoicePcm.wavPcm16le(wav)))
        assertEquals(100, VoicePcm.durationMs(pcm.size))
    }

    @Test
    fun rejectsEmptyOddAndOverlongPcm() {
        assertFailsWith<IllegalArgumentException> { VoicePcm.pcm16leToWav(ByteArray(0)) }
        assertFailsWith<IllegalArgumentException> { VoicePcm.pcm16leToWav(ByteArray(3)) }
        val tooLong = ByteArray(VoicePcm.SAMPLE_RATE * 2 * 61)
        assertFailsWith<IllegalArgumentException> { VoicePcm.pcm16leToWav(tooLong) }
    }

    @Test
    fun extraChunkIsKeptAndFoldedFromStereo48k() {
        val listed = byteArrayOf(
            'R'.code.toByte(), 'I'.code.toByte(), 'F'.code.toByte(), 'F'.code.toByte(),
            0, 0, 0, 0,
            'W'.code.toByte(), 'A'.code.toByte(), 'V'.code.toByte(), 'E'.code.toByte(),
            'f'.code.toByte(), 'm'.code.toByte(), 't'.code.toByte(), ' '.code.toByte(),
            16, 0, 0, 0,
            1, 0,
            2, 0,
            0x80.toByte(), 0xBB.toByte(), 0, 0,
            0, 0, 0, 0,
            4, 0,
            16, 0,
            'L'.code.toByte(), 'I'.code.toByte(), 'S'.code.toByte(), 'T'.code.toByte(),
            4, 0, 0, 0,
            'I'.code.toByte(), 'N'.code.toByte(), 'F'.code.toByte(), 'O'.code.toByte(),
            'd'.code.toByte(), 'a'.code.toByte(), 't'.code.toByte(), 'a'.code.toByte(),
            12, 0, 0, 0,
            0, 0, 0, 0,
            10, 0, 0, 0,
            20, 0, 0, 0,
        )
        val captured = assertNotNull(VoicePcm.readDeviceWav(listed))
        assertEquals(48_000, captured.rate)
        assertEquals(2, captured.channels)
        val folded = VoicePcm.toMono16k(captured.pcm, captured.rate, captured.channels)
        assertEquals(2, folded.size)
        assertEquals(10, folded[0].toInt() and 0xFF)
        assertEquals(0, folded[1].toInt() and 0xFF)
        assertNull(VoicePcm.wavPcm16le(listed))
    }

    @Test
    fun stereoHeaderIsNotPlayable() {
        val pcm = ByteArray(3_200)
        val wav = VoicePcm.pcm16leToWav(pcm)
        wav[22] = 2
        assertNull(VoicePcm.wavPcm16le(wav))
    }

    private fun readU16(bytes: ByteArray, offset: Int): Int =
        (bytes[offset].toInt() and 0xFF) or ((bytes[offset + 1].toInt() and 0xFF) shl 8)

    private fun readU32(bytes: ByteArray, offset: Int): Int =
        (bytes[offset].toInt() and 0xFF) or
            ((bytes[offset + 1].toInt() and 0xFF) shl 8) or
            ((bytes[offset + 2].toInt() and 0xFF) shl 16) or
            ((bytes[offset + 3].toInt() and 0xFF) shl 24)
}
