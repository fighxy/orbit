package com.orbit.desktop

import androidx.compose.ui.unit.dp
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

/**
 * Desktop entry point.
 *
 * - `-Dorbit.native.library=/abs/path/liborbit_ffi.so` selects the engine
 *   library (set by `./gradlew :apps:desktop:run`).
 * - `ORBIT_PROFILE` (or `-Dorbit.profile`) selects an isolated profile with its
 *   own data directory and keyring entry, for example to run two instances.
 */
fun main() {
    val profile = System.getProperty("orbit.profile") ?: System.getenv("ORBIT_PROFILE") ?: "default"
    val library = JniNativeLibrary.load(System.getProperty("orbit.native.library"))
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

    application {
        Window(
            title = if (profile == "default") "Orbit" else "Orbit — $profile",
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
