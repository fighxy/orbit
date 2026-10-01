package com.orbit.client.media

/**
 * 16 kHz mono 16-bit PCM WAV, the only voice note the core accepts.
 * Layout matches `encode_wav` in the Rust core: 44-byte header, then little-endian samples.
 */
object VoicePcm {
    const val SAMPLE_RATE = 16_000
    const val MAX_MS = 60_000
    const val MIN_MS = 100
    const val MAX_BYTES = 2 * 1024 * 1024

    fun durationMs(pcmBytes: Int): Int {
        if (pcmBytes < 2 || pcmBytes % 2 != 0) return 0
        return (pcmBytes / 2 * 1000L / SAMPLE_RATE).toInt()
    }

    fun pcm16leToWav(pcm: ByteArray): ByteArray {
        require(pcm.size % 2 == 0 && pcm.isNotEmpty()) { "pcm" }
        val duration = durationMs(pcm.size)
        require(duration in 1..MAX_MS) { "duration" }
        require(44L + pcm.size <= MAX_BYTES) { "size" }
        val out = ByteArray(44 + pcm.size)
        out[0] = 'R'.code.toByte()
        out[1] = 'I'.code.toByte()
        out[2] = 'F'.code.toByte()
        out[3] = 'F'.code.toByte()
        writeU32(out, 4, 36 + pcm.size)
        out[8] = 'W'.code.toByte()
        out[9] = 'A'.code.toByte()
        out[10] = 'V'.code.toByte()
        out[11] = 'E'.code.toByte()
        out[12] = 'f'.code.toByte()
        out[13] = 'm'.code.toByte()
        out[14] = 't'.code.toByte()
        out[15] = ' '.code.toByte()
        writeU32(out, 16, 16)
        writeU16(out, 20, 1)
        writeU16(out, 22, 1)
        writeU32(out, 24, SAMPLE_RATE)
        writeU32(out, 28, SAMPLE_RATE * 2)
        writeU16(out, 32, 2)
        writeU16(out, 34, 16)
        out[36] = 'd'.code.toByte()
        out[37] = 'a'.code.toByte()
        out[38] = 't'.code.toByte()
        out[39] = 'a'.code.toByte()
        writeU32(out, 40, pcm.size)
        pcm.copyInto(out, destinationOffset = 44)
        return out
    }

    /** PCM payload of a note this encoder produced. Null when the header is not 16 kHz mono PCM. */
    fun wavPcm16le(wav: ByteArray): ByteArray? {
        if (wav.size < 44 || tag(wav, 0) != "RIFF" || tag(wav, 8) != "WAVE") return null
        var format: ByteArray? = null
        var data: ByteArray? = null
        var cursor = 12
        while (cursor + 8 <= wav.size) {
            val id = tag(wav, cursor)
            val size = readU32(wav, cursor + 4)
            val start = cursor + 8
            val end = start + size
            if (size < 0 || end > wav.size) return null
            when (id) {
                "fmt " -> format = wav.copyOfRange(start, end)
                "data" -> data = wav.copyOfRange(start, end)
                else -> return null
            }
            cursor = end + (size % 2)
        }
        if (cursor != wav.size) return null
        val fmt = format ?: return null
        val pcm = data ?: return null
        if (fmt.size < 16) return null
        val audioFormat = readU16(fmt, 0)
        val channels = readU16(fmt, 2)
        val rate = readU32(fmt, 4)
        val bits = readU16(fmt, 14)
        if (audioFormat != 1 || channels != 1 || rate != SAMPLE_RATE || bits != 16) return null
        if (pcm.isEmpty() || pcm.size % 2 != 0) return null
        val duration = durationMs(pcm.size)
        if (duration !in 1..MAX_MS) return null
        return pcm
    }

    private fun tag(bytes: ByteArray, offset: Int): String =
        bytes.decodeToString(offset, offset + 4)

    private fun writeU16(out: ByteArray, offset: Int, value: Int) {
        out[offset] = value.toByte()
        out[offset + 1] = (value ushr 8).toByte()
    }

    private fun writeU32(out: ByteArray, offset: Int, value: Int) {
        out[offset] = value.toByte()
        out[offset + 1] = (value ushr 8).toByte()
        out[offset + 2] = (value ushr 16).toByte()
        out[offset + 3] = (value ushr 24).toByte()
    }

    private fun readU16(bytes: ByteArray, offset: Int): Int =
        (bytes[offset].toInt() and 0xFF) or ((bytes[offset + 1].toInt() and 0xFF) shl 8)

    private fun readU32(bytes: ByteArray, offset: Int): Int =
        (bytes[offset].toInt() and 0xFF) or
            ((bytes[offset + 1].toInt() and 0xFF) shl 8) or
            ((bytes[offset + 2].toInt() and 0xFF) shl 16) or
            ((bytes[offset + 3].toInt() and 0xFF) shl 24)
}
