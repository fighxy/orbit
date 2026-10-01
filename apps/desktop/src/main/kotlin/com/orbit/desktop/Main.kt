package com.orbit.desktop

import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.graphics.painter.BitmapPainter
import androidx.compose.ui.graphics.toComposeImageBitmap
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.type
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.WindowPlacement
import androidx.compose.ui.window.WindowPosition
import androidx.compose.ui.window.WindowState
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import com.orbit.client.app.AppController
import com.orbit.client.app.AppState
import com.orbit.client.app.OrbitApp
import com.orbit.client.app.PreferencesRepository
import com.orbit.client.app.PreferencesStorage
import com.orbit.client.app.ShellNavigation
import com.orbit.client.app.asGateway
import com.orbit.sdk.OrbitSdk
import com.orbit.sdk.bridge.JniNativeLibrary
import com.orbit.sdk.platform.DesktopPaths
import com.orbit.sdk.platform.DesktopSecretStore
import java.awt.Dimension
import java.io.File
import java.util.prefs.Preferences
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.runBlocking
import org.jetbrains.skia.Image

/**
 * Desktop entry point.
 *
 * - The engine library ships in the Compose app resources; for development
 *   `-Dorbit.native.library=/abs/path/liborbit_ffi.so` overrides it.
 * - `ORBIT_PROFILE` (or `-Dorbit.profile`) selects an isolated profile with its
 *   own data directory, keyring entry and interface settings.
 * - Shortcuts: Ctrl+, (Cmd+, on macOS) opens settings, Esc goes back.
 */
fun main() {
    val profile = System.getProperty("orbit.profile") ?: System.getenv("ORBIT_PROFILE") ?: "default"
    val library = JniNativeLibrary.load(nativeLibraryPath())
    val sdk = OrbitSdk(
        native = library,
        secretStore = DesktopSecretStore(library),
        dataDir = DesktopPaths.dataDir(profile).absolutePath,
        identityKey = "identity/$profile",
    )
    // Per-user store: the registry on Windows, ~/.java on Linux, plist on macOS.
    val storage = JavaPreferencesStorage(Preferences.userRoot().node("com/orbit/messenger/$profile"))
    val preferences = PreferencesRepository(storage)
    val navigation = ShellNavigation()
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    val controller = AppController(sdk.asGateway(), scope)
    controller.start()
    val linux = System.getProperty("os.name").lowercase().contains("linux")
    val appIcon = loadIcon()

    application {
        val windowState = rememberWindowState(
            placement = if (storage.read(KEY_MAXIMIZED) == "true") WindowPlacement.Maximized else WindowPlacement.Floating,
            position = storage.readPosition(),
            size = DpSize(storage.readDp(KEY_WIDTH, 1040f), storage.readDp(KEY_HEIGHT, 720f)),
        )
        Window(
            title = if (profile == "default") "Orbit" else "Orbit — $profile",
            icon = appIcon,
            state = windowState,
            onCloseRequest = {
                storage.writeWindow(windowState)
                // Close the engine so the storage lock is released before exit.
                runBlocking { controller.close() }
                exitApplication()
            },
            onKeyEvent = { event ->
                if (event.type != KeyEventType.KeyDown) return@Window false
                when {
                    event.key == Key.Comma && (event.isCtrlPressed || event.isMetaPressed) -> {
                        if (controller.state.value is AppState.Ready) navigation.settingsOpen = true
                        true
                    }
                    event.key == Key.Escape -> navigation.back()
                    else -> false
                }
            },
        ) {
            LaunchedEffect(Unit) { window.minimumSize = Dimension(420, 560) }
            OrbitApp(controller, preferences, navigation, linuxDesktop = linux)
        }
    }
}

private class JavaPreferencesStorage(private val node: Preferences) : PreferencesStorage {
    override fun read(key: String): String? = node.get(key, null)

    override fun write(key: String, value: String) {
        node.put(key, value)
        node.flush()
    }
}

private const val KEY_X = "window.x"
private const val KEY_Y = "window.y"
private const val KEY_WIDTH = "window.width"
private const val KEY_HEIGHT = "window.height"
private const val KEY_MAXIMIZED = "window.maximized"

private fun PreferencesStorage.readDp(key: String, default: Float) =
    (read(key)?.toFloatOrNull()?.takeIf { it >= 300f } ?: default).dp

private fun PreferencesStorage.readPosition(): WindowPosition {
    val x = read(KEY_X)?.toFloatOrNull() ?: return WindowPosition.PlatformDefault
    val y = read(KEY_Y)?.toFloatOrNull() ?: return WindowPosition.PlatformDefault
    return WindowPosition(x.dp, y.dp)
}

private fun PreferencesStorage.writeWindow(state: WindowState) {
    write(KEY_MAXIMIZED, (state.placement == WindowPlacement.Maximized).toString())
    if (state.placement != WindowPlacement.Floating) return
    write(KEY_WIDTH, state.size.width.value.toString())
    write(KEY_HEIGHT, state.size.height.value.toString())
    (state.position as? WindowPosition.Absolute)?.let {
        write(KEY_X, it.x.value.toString())
        write(KEY_Y, it.y.value.toString())
    }
}

/** Explicit override, else the copy bundled in the app resources, else the library path. */
private fun nativeLibraryPath(): String? =
    System.getProperty("orbit.native.library")
        ?: System.getProperty("compose.application.resources.dir")
            ?.let { File(it, System.mapLibraryName("orbit_ffi")) }
            ?.takeIf(File::isFile)
            ?.absolutePath

private fun loadIcon(): BitmapPainter? =
    Thread.currentThread().contextClassLoader.getResourceAsStream("orbit.png")?.use { stream ->
        BitmapPainter(Image.makeFromEncoded(stream.readAllBytes()).toComposeImageBitmap())
    }
