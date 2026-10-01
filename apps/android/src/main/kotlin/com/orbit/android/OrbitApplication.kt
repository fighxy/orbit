package com.orbit.android

import android.app.Application
import com.orbit.client.app.AppController
import com.orbit.client.app.asGateway
import com.orbit.sdk.OrbitSdk
import com.orbit.sdk.bridge.JniNativeLibrary
import com.orbit.sdk.platform.AndroidPaths
import com.orbit.sdk.platform.AndroidSecretStore
import kotlinx.coroutines.MainScope

/** Holds the engine for the whole process so it survives configuration changes. */
class OrbitApplication : Application() {
    lateinit var controller: AppController
        private set

    override fun onCreate() {
        super.onCreate()
        val library = JniNativeLibrary.load()
        val sdk = OrbitSdk(
            native = library,
            secretStore = AndroidSecretStore(this),
            dataDir = AndroidPaths.dataDir(this).absolutePath,
        )
        controller = AppController(sdk.asGateway(), MainScope())
        controller.start()
    }
}
