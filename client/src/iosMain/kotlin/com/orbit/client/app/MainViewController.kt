package com.orbit.client.app

import androidx.compose.ui.window.ComposeUIViewController
import com.orbit.client.designsystem.Strings
import com.orbit.client.media.IosAvatarPicker
import com.orbit.client.media.IosVoiceHost
import com.orbit.sdk.OrbitSdk
import com.orbit.sdk.bridge.IosNativeLibrary
import com.orbit.sdk.platform.IosPaths
import com.orbit.sdk.platform.KeychainSecretStore
import kotlinx.coroutines.MainScope
import platform.Foundation.NSUserDefaults
import platform.UIKit.UIViewController

/**
 * iOS entry point for the Xcode host: `MainViewControllerKt.MainViewController()`.
 * One engine per process; it stays open while the app runs.
 */
fun MainViewController(): UIViewController {
    val sdk = OrbitSdk(
        native = IosNativeLibrary,
        secretStore = KeychainSecretStore(),
        dataDir = IosPaths.dataDir(),
    )
    val controller = AppController(sdk.asGateway(), MainScope())
    controller.start()
    val preferences = PreferencesRepository(UserDefaultsStorage())
    val voice = IosVoiceHost()
    lateinit var root: UIViewController
    val avatars = IosAvatarPicker { root }
    root = ComposeUIViewController {
        OrbitApp(
            controller,
            preferences,
            prepareLocalNetwork = { true },
            localNetworkHint = Strings.localNetworkHintIos,
            voice = voice,
            pickAvatar = { avatars.pick() },
        )
    }
    return root
}

private class UserDefaultsStorage : PreferencesStorage {
    private val defaults = NSUserDefaults.standardUserDefaults

    override fun read(key: String): String? = defaults.stringForKey(key)

    override fun write(key: String, value: String) = defaults.setObject(value, forKey = key)
}
