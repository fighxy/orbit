package com.orbit.android

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import com.orbit.client.designsystem.Strings
import com.orbit.client.features.chat.VoiceActions
import com.orbit.client.features.chat.VoiceHost
import com.orbit.client.media.VoicePcm
import com.orbit.sdk.OrbitException
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.MessageId
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.ByteArrayOutputStream
import kotlin.coroutines.cancellation.CancellationException

/** Records and plays a 16 kHz mono 16-bit PCM note. No button is shown where this is absent. */
class AndroidVoiceHost(
    context: Context,
    private val ensureMic: suspend () -> Boolean,
) : VoiceHost {
    private val audioManager = context.applicationContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
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
    private var focus: AudioFocusRequest? = null

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
            val allowed = try {
                ensureMic()
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                false
            }
            if (gen != generation) return@launch
            if (!allowed) {
                errorState.value = Strings.voiceMicDenied
                return@launch
            }
            stopPlayback()
            if (gen != generation) return@launch
            val bufferSize = AudioRecord.getMinBufferSize(
                VoicePcm.SAMPLE_RATE,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
            )
            if (bufferSize <= 0) {
                errorState.value = Strings.voiceFailed
                return@launch
            }
            val chunk = bufferSize.coerceAtLeast(3_200)
            val recorder = try {
                AudioRecord.Builder()
                    .setAudioSource(MediaRecorder.AudioSource.MIC)
                    .setAudioFormat(
                        AudioFormat.Builder()
                            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                            .setSampleRate(VoicePcm.SAMPLE_RATE)
                            .setChannelMask(AudioFormat.CHANNEL_IN_MONO)
                            .build(),
                    )
                    .setBufferSizeInBytes(chunk)
                    .build()
            } catch (_: IllegalArgumentException) {
                null
            } catch (_: SecurityException) {
                null
            }
            if (recorder == null || recorder.state != AudioRecord.STATE_INITIALIZED) {
                recorder?.release()
                errorState.value = Strings.voiceFailed
                return@launch
            }
            if (gen != generation) {
                recorder.release()
                return@launch
            }
            requestFocus()
            val out = ByteArrayOutputStream()
            pcm = out
            stopRequested = false
            armed = actions
            armedConversation = conversationId
            recordingState.value = true
            elapsedState.value = 0
            try {
                recorder.startRecording()
            } catch (_: IllegalStateException) {
                recorder.release()
                recordingState.value = false
                errorState.value = Strings.voiceFailed
                return@launch
            }
            val maxBytes = VoicePcm.MAX_MS * VoicePcm.SAMPLE_RATE / 1000 * 2
            val thread = Thread({
                val buf = ByteArray(chunk)
                var posted = 0
                try {
                    while (!stopRequested) {
                        val read = recorder.read(buf, 0, buf.size)
                        if (read <= 0) break
                        val even = read - (read % 2)
                        var full = false
                        synchronized(out) {
                            val room = (maxBytes - out.size()).coerceAtLeast(0)
                            val take = even.coerceAtMost(room)
                            if (take > 0) out.write(buf, 0, take)
                            full = out.size() >= maxBytes
                        }
                        val ms = synchronized(out) { VoicePcm.durationMs(out.size()) }
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
                    runCatching { recorder.stop() }
                    recorder.release()
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
            playingState.value = messageId.hex
            playStop = false
            requestFocus()
            val thread = Thread({
                val min = AudioTrack.getMinBufferSize(
                    VoicePcm.SAMPLE_RATE,
                    AudioFormat.CHANNEL_OUT_MONO,
                    AudioFormat.ENCODING_PCM_16BIT,
                ).coerceAtLeast(3_200)
                val track = AudioTrack.Builder()
                    .setAudioAttributes(
                        AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_MEDIA)
                            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                            .build(),
                    )
                    .setAudioFormat(
                        AudioFormat.Builder()
                            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                            .setSampleRate(VoicePcm.SAMPLE_RATE)
                            .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                            .build(),
                    )
                    .setBufferSizeInBytes(min)
                    .setTransferMode(AudioTrack.MODE_STREAM)
                    .build()
                try {
                    track.play()
                    var offset = 0
                    while (!playStop && offset < samples.size) {
                        val wrote = track.write(samples, offset, minOf(min, samples.size - offset))
                        if (wrote <= 0) break
                        offset += wrote
                    }
                    val sampleCount = samples.size / 2
                    while (!playStop && track.playbackHeadPosition < sampleCount) {
                        Thread.sleep(40)
                    }
                } catch (_: IllegalStateException) {
                    scope.launch { errorState.value = Strings.voiceFailed }
                } finally {
                    runCatching { track.pause() }
                    runCatching { track.flush() }
                    runCatching { track.release() }
                    scope.launch {
                        if (playingState.value == messageId.hex) playingState.value = null
                        if (!recordingState.value) abandonFocus()
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
        synchronized(this) {
            if (finishing) return
            if (!recordingState.value && recordThread == null) return
            finishing = true
            stopRequested = true
            thread = recordThread
            buffer = pcm
            target = armedConversation
            sink = armed
            recordingState.value = false
        }
        scope.launch(Dispatchers.Default) {
            runCatching { thread?.join(2_000) }
            val raw = synchronized(buffer) { buffer.toByteArray() }
            val even = if (raw.size % 2 == 0) raw else raw.copyOf(raw.size - 1)
            withContext(Dispatchers.Main) {
                elapsedState.value = 0
                recordThread = null
                finishing = false
                armed = null
                armedConversation = null
                if (!playingState.value.isNullOrEmpty()) Unit else abandonFocus()
                if (!send || target == null || sink == null) return@withContext
                if (VoicePcm.durationMs(even.size) < VoicePcm.MIN_MS) {
                    errorState.value = Strings.voiceTooShort
                    return@withContext
                }
                val wav = runCatching { VoicePcm.pcm16leToWav(even) }.getOrElse {
                    errorState.value = Strings.voiceFailed
                    return@withContext
                }
                sink.sendVoice(target, wav)
            }
        }
    }

    private fun stopPlayback() {
        playStop = true
        val thread = playThread
        playThread = null
        thread?.let { runner ->
            scope.launch(Dispatchers.Default) { runCatching { runner.join(1_000) } }
        }
        playingState.value = null
    }

    private fun requestFocus() {
        val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .build()
        focus = request
        audioManager.requestAudioFocus(request)
    }

    private fun abandonFocus() {
        focus?.let { audioManager.abandonAudioFocusRequest(it) }
        focus = null
    }
}
