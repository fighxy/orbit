package com.orbit.desktop

import androidx.compose.ui.graphics.painter.BitmapPainter
import androidx.compose.ui.graphics.toComposeImageBitmap
import androidx.compose.ui.unit.dp
import java.io.File
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import com.orbit.client.app.AppController
import com.orbit.client.app.OrbitApp
import com.orbit.client.app.asGateway
import com.orbit.sdk.OrbitSdk
import com.orbit.sdk.bridge.JniNativeLibrary
import com.orbit.sdk.platform.DesktopPaths
import com.orbit.sdk.platform.DesktopSecretStore
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
 *   own data directory and keyring entry, for example to run two instances.
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
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    val controller = AppController(sdk.asGateway(), scope)
    controller.start()
    val linux = System.getProperty("os.name").lowercase().contains("linux")
    val appIcon = loadIcon()

    application {
        Window(
            title = if (profile == "default") "Orbit" else "Orbit — $profile",
            icon = appIcon,
            state = rememberWindowState(width = 1040.dp, height = 720.dp),
            onCloseRequest = {
                // Close the engine so the storage lock is released before exit.
                runBlocking { controller.close() }
                exitApplication()
            },
        ) {
            OrbitApp(controller, linuxDesktop = linux)
        }
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
