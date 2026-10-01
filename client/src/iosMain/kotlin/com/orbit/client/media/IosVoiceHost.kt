package com.orbit.client.media

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.orbit.client.designsystem.Strings
import com.orbit.client.features.chat.VoiceActions
import com.orbit.client.features.chat.VoiceHost
import com.orbit.sdk.OrbitException
import com.orbit.sdk.model.ConversationId
import com.orbit.sdk.model.MessageId
import kotlin.coroutines.resume
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import platform.AVFAudio.AVAudioPlayer
import platform.AVFAudio.AVAudioPlayerDelegateProtocol
import platform.AVFAudio.AVAudioRecorder
import platform.AVFAudio.AVAudioSession
import platform.AVFAudio.AVAudioSessionCategoryPlayAndRecord
import platform.AVFAudio.AVAudioSessionPortOverrideSpeaker
import platform.AVFAudio.AVAudioSessionRecordPermissionDenied
import platform.AVFAudio.AVAudioSessionRecordPermissionGranted
import platform.AVFAudio.AVFormatIDKey
import platform.AVFAudio.AVLinearPCMBitDepthKey
import platform.AVFAudio.AVLinearPCMIsBigEndianKey
import platform.AVFAudio.AVLinearPCMIsFloatKey
import platform.AVFAudio.AVNumberOfChannelsKey
import platform.AVFAudio.AVSampleRateKey
import platform.CoreAudioTypes.kAudioFormatLinearPCM
import platform.Foundation.NSData
import platform.Foundation.NSFileManager
import platform.Foundation.NSNotificationCenter
import platform.Foundation.NSNumber
import platform.Foundation.NSOperationQueue
import platform.Foundation.NSTemporaryDirectory
import platform.Foundation.NSURL
import platform.UIKit.UIApplicationDidEnterBackgroundNotification
import platform.darwin.NSObject
import platform.darwin.NSObjectProtocol

/**
 * Records with AVAudioRecorder and plays with AVAudioPlayer.
 * AVAudioApplication is iOS 17 only; this app still runs on iOS 16, so permission
 * goes through AVAudioSession. A device that writes 32 or 48 kHz, stereo, or an
 * extra LIST/fact chunk is folded to the canonical 16 kHz mono WAV.
 */
class IosVoiceHost : VoiceHost {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var recordingState by mutableStateOf(false)
    private var elapsedState by mutableStateOf(0)
    private var playingState by mutableStateOf<String?>(null)
    private var errorState by mutableStateOf<String?>(null)

    private var actions: VoiceActions? = null
    private var armed: VoiceActions? = null
    private var armedConversation: ConversationId? = null
    private var generation = 0
    private var recorder: AVAudioRecorder? = null
    private var recordPath: String? = null
    private var player: AVAudioPlayer? = null
    private var playerDelegate: NSObject? = null
    private var backgroundObserver: NSObjectProtocol? = null

    override val recording: Boolean get() = recordingState
    override val elapsedMs: Int get() = elapsedState
    override val playingHex: String? get() = playingState
    override val error: String? get() = errorState

    init {
        backgroundObserver = NSNotificationCenter.defaultCenter.addObserverForName(
            name = UIApplicationDidEnterBackgroundNotification,
            `object` = null,
            queue = NSOperationQueue.mainQueue,
            usingBlock = { _ -> commit() },
        )
    }

    override fun attach(actions: VoiceActions?) {
        this.actions = actions
    }

    override fun dismissError() {
        errorState = null
    }

    override fun toggleRecord(conversationId: ConversationId) {
        if (recordingState || recorder != null) {
            generation += 1
            finish(send = true)
            return
        }
        val gen = ++generation
        errorState = null
        scope.launch {
            val allowed = try {
                ensureMic()
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (_: Exception) {
                false
            }
            if (gen != generation) return@launch
            if (!allowed) {
                errorState = Strings.voiceMicDenied
                return@launch
            }
            stopPlayback()
            if (gen != generation) return@launch
            if (!activateSpeaker()) {
                errorState = Strings.voiceFailed
                return@launch
            }
            val url = takeUrl(gen)
            val path = url?.path
            if (url == null || path == null) {
                errorState = Strings.voiceFailed
                return@launch
            }
            val created = AVAudioRecorder(url, recordSettings(), null)
            if (created == null || !created.prepareToRecord() || !created.record()) {
                deletePath(path)
                errorState = Strings.voiceFailed
                return@launch
            }
            if (gen != generation) {
                created.stop()
                deletePath(path)
                return@launch
            }
            recorder = created
            recordPath = path
            armed = actions
            armedConversation = conversationId
            recordingState = true
            elapsedState = 0
            while (gen == generation && recordingState) {
                val ms = (created.currentTime * 1000.0).toInt()
                elapsedState = ms.coerceAtMost(VoicePcm.MAX_MS)
                if (ms >= VoicePcm.MAX_MS) {
                    generation += 1
                    finish(send = true)
                    return@launch
                }
                delay(100)
            }
        }
    }

    override fun commit() {
        if (!recordingState && recorder == null) return
        generation += 1
        finish(send = true)
    }

    override fun togglePlay(messageId: MessageId) {
        if (playingState == messageId.hex) {
            stopPlayback()
            return
        }
        if (recordingState || recorder != null) return
        stopPlayback()
        val sink = actions ?: return
        errorState = null
        scope.launch {
            val wav = try {
                sink.readVoice(messageId)
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: OrbitException) {
                errorState = Strings.describe(e)
                return@launch
            } catch (_: IllegalArgumentException) {
                errorState = Strings.voiceFailed
                return@launch
            }
            if (VoicePcm.wavPcm16le(wav) == null) {
                errorState = Strings.voiceFailed
                return@launch
            }
            if (recordingState || recorder != null) return@launch
            if (!activateSpeaker()) {
                errorState = Strings.voiceFailed
                return@launch
            }
            val delegate = PlaybackDelegate(
                onEnd = {
                    if (playingState == messageId.hex) playingState = null
                },
                onError = {
                    if (playingState == messageId.hex) playingState = null
                    errorState = Strings.voiceFailed
                },
            )
            val audio = AVAudioPlayer(wav.toNSData(), null)
            if (audio == null) {
                errorState = Strings.voiceFailed
                return@launch
            }
            playerDelegate = delegate
            audio.delegate = delegate
            audio.prepareToPlay()
            if (!audio.play()) {
                playerDelegate = null
                errorState = Strings.voiceFailed
                return@launch
            }
            player = audio
            playingState = messageId.hex
        }
    }

    override fun release() {
        generation += 1
        finish(send = false)
        stopPlayback()
        backgroundObserver?.let { NSNotificationCenter.defaultCenter.removeObserver(it) }
        backgroundObserver = null
        scope.cancel()
    }

    private fun finish(send: Boolean) {
        val active = recorder
        val path = recordPath
        val target = armedConversation
        val sink = armed
        if (active == null && !recordingState) return
        recorder = null
        recordPath = null
        armed = null
        armedConversation = null
        recordingState = false
        elapsedState = 0
        if (active != null) runCatching { active.stop() }
        if (path == null) return
        try {
            if (!send || target == null || sink == null) return
            when (val take = readTake(path)) {
                is Take.Ready -> sink.sendVoice(target, take.wav)
                Take.TooShort -> errorState = Strings.voiceTooShort
                Take.Failed -> errorState = Strings.voiceFailed
            }
        } finally {
            deletePath(path)
        }
    }

    private fun readTake(path: String): Take {
        val data = NSData.dataWithContentsOfFile(path) ?: return Take.Failed
        if (data.length > (12L * 1024 * 1024).toULong()) return Take.Failed
        val device = VoicePcm.readDeviceWav(data.toByteArray()) ?: return Take.Failed
        val folded = runCatching {
            VoicePcm.toMono16k(device.pcm, device.rate, device.channels)
        }.getOrNull() ?: return Take.Failed
        val maxBytes = VoicePcm.SAMPLE_RATE * VoicePcm.MAX_MS / 1000 * 2
        val even = folded.size - folded.size % 2
        val pcm = if (even > maxBytes) folded.copyOf(maxBytes) else folded.copyOf(even)
        if (VoicePcm.durationMs(pcm.size) < VoicePcm.MIN_MS) return Take.TooShort
        val wav = runCatching { VoicePcm.pcm16leToWav(pcm) }.getOrNull() ?: return Take.Failed
        return Take.Ready(wav)
    }

    private fun stopPlayback() {
        val active = player
        player = null
        playerDelegate = null
        active?.delegate = null
        runCatching { active?.stop() }
        playingState = null
    }

    private suspend fun ensureMic(): Boolean {
        val session = AVAudioSession.sharedInstance()
        val permission = session.recordPermission
        if (permission == AVAudioSessionRecordPermissionGranted) return true
        if (permission == AVAudioSessionRecordPermissionDenied) return false
        return suspendCancellableCoroutine { cont ->
            session.requestRecordPermission { granted ->
                if (cont.isActive) cont.resume(granted)
            }
        }
    }

    private fun activateSpeaker(): Boolean {
        val session = AVAudioSession.sharedInstance()
        if (!session.setCategory(AVAudioSessionCategoryPlayAndRecord, error = null)) return false
        session.setPreferredSampleRate(VoicePcm.SAMPLE_RATE.toDouble(), error = null)
        session.overrideOutputAudioPort(AVAudioSessionPortOverrideSpeaker, error = null)
        return session.setActive(true, error = null)
    }

    private fun takeUrl(gen: Int): NSURL? {
        val base = NSURL.fileURLWithPath(NSTemporaryDirectory(), isDirectory = true)
        return base.URLByAppendingPathComponent("orbit-voice-$gen.wav")
    }

    private fun deletePath(path: String) {
        NSFileManager.defaultManager.removeItemAtPath(path, error = null)
    }

    private fun recordSettings(): Map<Any?, *> = mapOf(
        AVFormatIDKey to NSNumber.numberWithUnsignedInt(kAudioFormatLinearPCM),
        AVSampleRateKey to NSNumber.numberWithDouble(VoicePcm.SAMPLE_RATE.toDouble()),
        AVNumberOfChannelsKey to NSNumber.numberWithInt(1),
        AVLinearPCMBitDepthKey to NSNumber.numberWithInt(16),
        AVLinearPCMIsBigEndianKey to NSNumber.numberWithBool(false),
        AVLinearPCMIsFloatKey to NSNumber.numberWithBool(false),
    )

    private class PlaybackDelegate(
        private val onEnd: () -> Unit,
        private val onError: () -> Unit,
    ) : NSObject(), AVAudioPlayerDelegateProtocol {
        override fun audioPlayerDidFinishPlaying(player: AVAudioPlayer, successfully: Boolean) {
            if (successfully) onEnd() else onError()
        }
    }

    private sealed interface Take {
        class Ready(val wav: ByteArray) : Take
        data object TooShort : Take
        data object Failed : Take
    }
}
