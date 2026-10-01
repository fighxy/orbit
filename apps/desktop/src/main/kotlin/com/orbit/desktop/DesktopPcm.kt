package com.orbit.desktop

import javax.sound.sampled.AudioFormat

/** Capture layout the desktop recorder can fold into a 16 kHz mono note. */
internal data class PcmLayout(val rate: Int, val channels: Int)

internal fun matchesVoiceCapture(format: AudioFormat): PcmLayout? {
    if (format.encoding != AudioFormat.Encoding.PCM_SIGNED) return null
    if (format.sampleSizeInBits != 16 || format.isBigEndian) return null
    val channels = format.channels
    if (channels != 1 && channels != 2) return null
    val rate = format.sampleRate.toInt()
    if (rate != 16_000 && rate != 32_000 && rate != 48_000) return null
    if (format.sampleRate.toInt().toFloat() != format.sampleRate) return null
    return PcmLayout(rate, channels)
}

/** Left channel, then an integer fold down to 16 kHz. */
internal fun toMono16k(pcm: ByteArray, layout: PcmLayout): ByteArray =
    foldTo16k(downmixLeft(pcm, layout.channels), layout.rate)

internal fun downmixLeft(pcm: ByteArray, channels: Int): ByteArray {
    require(channels == 1 || channels == 2)
    if (channels == 1) return pcm.copyOf(pcm.size - pcm.size % 2)
    val frames = pcm.size / 4
    val out = ByteArray(frames * 2)
    var input = 0
    var output = 0
    repeat(frames) {
        out[output] = pcm[input]
        out[output + 1] = pcm[input + 1]
        input += 4
        output += 2
    }
    return out
}

internal fun foldTo16k(pcm: ByteArray, sourceRate: Int): ByteArray {
    require(sourceRate == 16_000 || sourceRate == 32_000 || sourceRate == 48_000)
    val even = pcm.size - pcm.size % 2
    if (sourceRate == 16_000) return pcm.copyOf(even)
    val factor = sourceRate / 16_000
    val outSamples = (even / 2) / factor
    val out = ByteArray(outSamples * 2)
    var input = 0
    for (sample in 0 until outSamples) {
        var sum = 0
        repeat(factor) {
            val lo = pcm[input].toInt() and 0xFF
            val hi = pcm[input + 1].toInt() and 0xFF
            sum += (lo or (hi shl 8)).toShort().toInt()
            input += 2
        }
        val average = sum / factor
        out[sample * 2] = (average and 0xFF).toByte()
        out[sample * 2 + 1] = ((average shr 8) and 0xFF).toByte()
    }
    return out
}

/** 16 kHz mono samples reshaped to the rate and channel count the opened speaker line reports. */
internal fun toSpeaker(pcm16k: ByteArray, layout: PcmLayout): ByteArray {
    val atRate = if (layout.rate == 16_000) {
        pcm16k.copyOf(pcm16k.size - pcm16k.size % 2)
    } else {
        expandFrom16k(pcm16k, layout.rate)
    }
    if (layout.channels == 1) return atRate
    val samples = atRate.size / 2
    val out = ByteArray(samples * 4)
    var output = 0
    for (sample in 0 until samples) {
        val lo = atRate[sample * 2]
        val hi = atRate[sample * 2 + 1]
        out[output] = lo
        out[output + 1] = hi
        out[output + 2] = lo
        out[output + 3] = hi
        output += 4
    }
    return out
}

/** Repeats each 16 kHz sample so a speaker that only opens at 32 or 48 kHz can play the note. */
internal fun expandFrom16k(pcm: ByteArray, targetRate: Int): ByteArray {
    val factor = targetRate / 16_000
    require(targetRate == 16_000 || targetRate == 32_000 || targetRate == 48_000)
    val samples = pcm.size / 2
    if (factor == 1) return pcm.copyOf(samples * 2)
    val out = ByteArray(samples * factor * 2)
    var output = 0
    for (sample in 0 until samples) {
        val lo = pcm[sample * 2]
        val hi = pcm[sample * 2 + 1]
        repeat(factor) {
            out[output] = lo
            out[output + 1] = hi
            output += 2
        }
    }
    return out
}
