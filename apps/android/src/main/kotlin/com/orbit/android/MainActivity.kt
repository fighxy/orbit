package com.orbit.android

import android.os.Bundle
import android.os.Build
import android.Manifest
import android.content.pm.PackageManager
import android.net.Uri
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.remember
import com.orbit.client.app.OrbitApp
import com.orbit.client.app.ShellNavigation
import com.orbit.client.designsystem.Strings
import com.orbit.client.features.settings.AvatarPick
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.coroutines.cancellation.CancellationException

class MainActivity : ComponentActivity() {
    private var permissionResult: CompletableDeferred<Boolean>? = null
    private val localNetworkPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        permissionResult?.complete(granted)
        permissionResult = null
    }
    private var micResult: CompletableDeferred<Boolean>? = null
    private val micPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        micResult?.complete(granted)
        micResult = null
    }
    private var avatarResult: CompletableDeferred<Uri?>? = null
    private val pickImage = registerForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        avatarResult?.complete(uri)
        avatarResult = null
    }
    private val voice by lazy { AndroidVoiceHost(this, ::requestMic) }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val app = application as OrbitApplication
        val voiceHost = voice
        setContent {
            val navigation = remember { ShellNavigation() }
            BackHandler(enabled = navigation.handlesBack) { navigation.back() }
            OrbitApp(
                app.controller,
                app.preferences,
                navigation,
                prepareLocalNetwork = if (Build.VERSION.SDK_INT >= 37) ({ requestLocalNetwork() }) else null,
                voice = voiceHost,
                pickAvatar = { pickAvatar() },
            )
        }
    }

    // SDK 37 gates native UDP to LAN nodes. Ask in the connection flow, never
    // during local account creation. Denial must leave public Internet usable.
    private suspend fun requestLocalNetwork(): Boolean {
        if (Build.VERSION.SDK_INT < 37 || checkSelfPermission(Manifest.permission.ACCESS_LOCAL_NETWORK) == PackageManager.PERMISSION_GRANTED) return true
        permissionResult?.let { return it.await() }
        val result = CompletableDeferred<Boolean>()
        permissionResult = result
        localNetworkPermission.launch(Manifest.permission.ACCESS_LOCAL_NETWORK)
        return result.await()
    }

    private suspend fun requestMic(): Boolean {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            return true
        }
        micResult?.let { return it.await() }
        val result = CompletableDeferred<Boolean>()
        micResult = result
        micPermission.launch(Manifest.permission.RECORD_AUDIO)
        return result.await()
    }

    private suspend fun pickAvatar(): AvatarPick {
        avatarResult?.cancel()
        val result = CompletableDeferred<Uri?>()
        avatarResult = result
        pickImage.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
        val uri = result.await() ?: return AvatarPick.Cancelled
        return withContext(Dispatchers.Default) {
            try {
                scaleAvatarJpeg(contentResolver, uri)
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                AvatarPick.Rejected(Strings.avatarUnreadable)
            }
        }
    }

    override fun onDestroy() {
        permissionResult?.cancel()
        permissionResult = null
        micResult?.cancel()
        micResult = null
        avatarResult?.cancel()
        avatarResult = null
        voice.release()
        super.onDestroy()
    }
}
