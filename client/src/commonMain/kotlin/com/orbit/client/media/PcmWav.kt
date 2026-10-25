package com.orbit.client.media

/**
 * 16 kHz mono 16-bit PCM WAV, the only voice note the core accepts.
 * Layout matches `encode_wav` in the Rust core: 44-byte header, then little-endian samples.
 */

/** PCM a device recorder wrote. Wider than the canonical 16 kHz mono note. */
class DeviceWav(
    val rate: Int,
    val channels: Int,
    val pcm: ByteArray,
)

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

    /**
     * PCM from a device WAV. Unknown chunks are skipped, unlike [wavPcm16le],
     * because a recorder may insert LIST or fact. Only 16-bit little-endian PCM
     * at 16, 32, or 48 kHz, mono or stereo, is accepted.
     */
    fun readDeviceWav(wav: ByteArray): DeviceWav? {
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
            }
            cursor = end + (size and 1)
        }
        val fmt = format ?: return null
        val pcm = data ?: return null
        if (fmt.size < 16 || pcm.isEmpty() || pcm.size % 2 != 0 || pcm.size > 12 * 1024 * 1024) return null
        val audioFormat = readU16(fmt, 0)
        val channels = readU16(fmt, 2)
        val rate = readU32(fmt, 4)
        val bits = readU16(fmt, 14)
        if (audioFormat != 1 || bits != 16) return null
        if (channels != 1 && channels != 2) return null
        if (rate != SAMPLE_RATE && rate != 32_000 && rate != 48_000) return null
        return DeviceWav(rate, channels, pcm)
    }

    /** Left channel, then an integer average down to 16 kHz. */
    fun toMono16k(pcm: ByteArray, rate: Int, channels: Int): ByteArray {
        require(channels == 1 || channels == 2)
        require(rate == SAMPLE_RATE || rate == 32_000 || rate == 48_000)
        val mono = if (channels == 1) {
            pcm.copyOf(pcm.size - pcm.size % 2)
        } else {
            val frames = pcm.size / 4
            ByteArray(frames * 2).also { out ->
                var input = 0
                var output = 0
                repeat(frames) {
                    out[output] = pcm[input]
                    out[output + 1] = pcm[input + 1]
                    input += 4
                    output += 2
                }
            }
        }
        if (rate == SAMPLE_RATE) return mono
        val factor = rate / SAMPLE_RATE
        val samples = (mono.size / 2) / factor
        val out = ByteArray(samples * 2)
        var input = 0
        for (sample in 0 until samples) {
            var sum = 0
            repeat(factor) {
                val lo = mono[input].toInt() and 0xFF
                val hi = mono[input + 1].toInt() and 0xFF
                sum += (lo or (hi shl 8)).toShort().toInt()
                input += 2
            }
            val average = sum / factor
            out[sample * 2] = (average and 0xFF).toByte()
            out[sample * 2 + 1] = ((average shr 8) and 0xFF).toByte()
        }
        return out
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
