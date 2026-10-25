package com.orbit.desktop

import com.orbit.client.designsystem.Strings
import com.orbit.client.features.chat.VoiceActions
import com.orbit.client.features.chat.VoiceHost
import com.orbit.client.media.VoicePcm
import com.orbit.sdk.OrbitException
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.MessageId
import java.io.ByteArrayOutputStream
import javax.sound.sampled.AudioFormat
import javax.sound.sampled.AudioSystem
import javax.sound.sampled.DataLine
import javax.sound.sampled.SourceDataLine
import javax.sound.sampled.TargetDataLine
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Records and plays a 16 kHz mono note through the JVM mixer. No button is shown without this. */
class DesktopVoiceHost : VoiceHost {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val recordingState = androidx.compose.runtime.mutableStateOf(false)
    private val elapsedState = androidx.compose.runtime.mutableStateOf(0)
    private val playingState = androidx.compose.runtime.mutableStateOf<String?>(null)
    private val errorState = androidx.compose.runtime.mutableStateOf<String?>(null)

    private var actions: VoiceActions? = null
    private var armed: VoiceActions? = null
    private var armedConversation: ConversationId? = null
    private var generation = 0

    @Volatile private var stopRequested = false
    private var pcm = ByteArrayOutputStream()
    private var recordThread: Thread? = null
    private var finishing = false

    @Volatile private var playStop = false
    private var playThread: Thread? = null

    override val recording: Boolean get() = recordingState.value
    override val elapsedMs: Int get() = elapsedState.value
    override val playingHex: String? get() = playingState.value
    override val error: String? get() = errorState.value

    override fun attach(actions: VoiceActions?) {
        this.actions = actions
    }

    override fun dismissError() {
        errorState.value = null
    }

    override fun toggleRecord(conversationId: ConversationId) {
        if (recordingState.value) {
            generation += 1
            finish(send = true)
            return
        }
        val gen = ++generation
        errorState.value = null
        scope.launch {
            val opened = withContext(Dispatchers.Default) { openCapture() }
            if (gen != generation) {
                opened?.close()
                return@launch
            }
            if (opened == null) {
                errorState.value = Strings.voiceFailed
                return@launch
            }
            stopPlayback()
            if (gen != generation) {
                opened.close()
                return@launch
            }
            val out = ByteArrayOutputStream()
            pcm = out
            stopRequested = false
            armed = actions
            armedConversation = conversationId
            recordingState.value = true
            elapsedState.value = 0
            captureLayout = opened.layout
            val layout = opened.layout
            val frame = layout.channels * 2
            val maxBytes = VoicePcm.MAX_MS * layout.rate / 1000 * frame
            val thread = Thread({
                val buf = ByteArray((layout.rate / 10 * frame).coerceAtLeast(frame))
                var posted = 0
                try {
                    opened.line.start()
                    while (!stopRequested) {
                        val read = opened.line.read(buf, 0, buf.size)
                        if (read <= 0) break
                        val whole = read - (read % frame)
                        var full = false
                        synchronized(out) {
                            val room = (maxBytes - out.size()).coerceAtLeast(0)
                            val take = whole.coerceAtMost(room)
                            if (take > 0) out.write(buf, 0, take)
                            full = out.size() >= maxBytes
                        }
                        val ms = synchronized(out) { capturedMs(out.size(), layout) }
                        if (ms - posted >= 100) {
                            posted = ms
                            scope.launch { elapsedState.value = ms }
                        }
                        if (full) {
                            scope.launch {
                                generation += 1
                                finish(send = true)
                            }
                            break
                        }
                    }
                } finally {
                    opened.close()
                }
            }, "orbit-mic")
            recordThread = thread
            thread.start()
        }
    }

    override fun commit() {
        generation += 1
        if (recordingState.value || recordThread != null) finish(send = true)
    }

    override fun togglePlay(messageId: MessageId) {
        if (playingState.value == messageId.hex) {
            stopPlayback()
            return
        }
        if (recordingState.value) return
        stopPlayback()
        val sink = actions ?: return
        errorState.value = null
        scope.launch {
            val wav = try {
                sink.readVoice(messageId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: OrbitException) {
                errorState.value = Strings.describe(e)
                return@launch
            } catch (_: IllegalArgumentException) {
                errorState.value = Strings.voiceFailed
                return@launch
            }
            val samples = VoicePcm.wavPcm16le(wav)
            if (samples == null) {
                errorState.value = Strings.voiceFailed
                return@launch
            }
            if (recordingState.value) return@launch
            val speaker = withContext(Dispatchers.Default) { openSpeaker(samples) }
            if (speaker == null) {
                errorState.value = Strings.voiceFailed
                return@launch
            }
            playingState.value = messageId.hex
            playStop = false
            val thread = Thread({
                try {
                    speaker.line.start()
                    var offset = 0
                    val chunk = (speaker.layout.rate / 10 * speaker.layout.channels * 2).coerceAtLeast(speaker.layout.channels * 2)
                    while (!playStop && offset < speaker.pcm.size) {
                        val count = minOf(chunk, speaker.pcm.size - offset)
                        val wrote = speaker.line.write(speaker.pcm, offset, count)
                        if (wrote <= 0) break
                        offset += wrote
                    }
                    if (!playStop) speaker.line.drain()
                } finally {
                    runCatching { speaker.line.stop() }
                    runCatching { speaker.line.flush() }
                    runCatching { speaker.line.close() }
                    scope.launch {
                        if (playingState.value == messageId.hex) playingState.value = null
                    }
                }
            }, "orbit-speaker")
            playThread = thread
            thread.start()
        }
    }

    override fun release() {
        generation += 1
        finish(send = false)
        stopPlayback()
        scope.cancel()
    }

    private fun finish(send: Boolean) {
        val thread: Thread?
        val buffer: ByteArrayOutputStream
        val target: ConversationId?
        val sink: VoiceActions?
        val layout: PcmLayout?
        synchronized(this) {
            if (finishing) return
            if (!recordingState.value && recordThread == null) return
            finishing = true
            stopRequested = true
            thread = recordThread
            buffer = pcm
            target = armedConversation
            sink = armed
            layout = captureLayout
            recordingState.value = false
        }
        scope.launch(Dispatchers.Default) {
            runCatching { thread?.join(2_000) }
            val raw = synchronized(buffer) { buffer.toByteArray() }
            val folded = if (layout == null) ByteArray(0) else runCatching { toMono16k(raw, layout) }.getOrDefault(ByteArray(0))
            withContext(Dispatchers.Main) {
                elapsedState.value = 0
                recordThread = null
                captureLayout = null
                finishing = false
                armed = null
                armedConversation = null
                if (!send || target == null || sink == null) return@withContext
                if (VoicePcm.durationMs(folded.size) < VoicePcm.MIN_MS) {
                    errorState.value = Strings.voiceTooShort
                    return@withContext
                }
                val wav = runCatching { VoicePcm.pcm16leToWav(folded) }.getOrElse {
                    errorState.value = Strings.voiceFailed
                    return@withContext
                }
                sink.sendVoice(target, wav)
            }
        }
    }

    private var captureLayout: PcmLayout? = null

    private fun stopPlayback() {
        playStop = true
        val thread = playThread
        playThread = null
        thread?.let { runner ->
            scope.launch(Dispatchers.Default) { runCatching { runner.join(1_000) } }
        }
        playingState.value = null
    }

    private fun openCapture(): OpenCapture? {
        val rates = intArrayOf(16_000, 48_000, 32_000)
        for (mixerInfo in AudioSystem.getMixerInfo()) {
            val mixer = AudioSystem.getMixer(mixerInfo)
            for (rate in rates) {
                val requested = AudioFormat(rate.toFloat(), 16, 1, true, false)
                val info = DataLine.Info(TargetDataLine::class.java, requested)
                if (!mixer.isLineSupported(info)) continue
                val line = try {
                    mixer.getLine(info) as TargetDataLine
                } catch (_: Exception) {
                    continue
                }
                try {
                    line.open(requested, rate / 5 * 2)
                } catch (_: Exception) {
                    runCatching { line.close() }
                    continue
                }
                val layout = matchesVoiceCapture(line.format)
                if (layout == null) {
                    line.close()
                    continue
                }
                return OpenCapture(line, layout)
            }
        }
        return null
    }

    private fun openSpeaker(pcm16k: ByteArray): OpenSpeaker? {
        val rates = intArrayOf(16_000, 48_000, 32_000)
        for (mixerInfo in AudioSystem.getMixerInfo()) {
            val mixer = AudioSystem.getMixer(mixerInfo)
            for (rate in rates) {
                val requested = AudioFormat(rate.toFloat(), 16, 1, true, false)
                val info = DataLine.Info(SourceDataLine::class.java, requested)
                if (!mixer.isLineSupported(info)) continue
                val line = try {
                    mixer.getLine(info) as SourceDataLine
                } catch (_: Exception) {
                    continue
                }
                try {
                    line.open(requested, rate / 5 * 2)
                } catch (_: Exception) {
                    runCatching { line.close() }
                    continue
                }
                val layout = matchesVoiceCapture(line.format)
                if (layout == null) {
                    line.close()
                    continue
                }
                return OpenSpeaker(line, layout, toSpeaker(pcm16k, layout))
            }
        }
        return null
    }
}

private class OpenCapture(val line: TargetDataLine, val layout: PcmLayout) {
    fun close() {
        runCatching { line.stop() }
        runCatching { line.close() }
    }
}

private class OpenSpeaker(val line: SourceDataLine, val layout: PcmLayout, val pcm: ByteArray)

private fun capturedMs(bytes: Int, layout: PcmLayout): Int {
    val samples = bytes / (layout.channels * 2)
    return (samples * 1000L / layout.rate).toInt()
}
