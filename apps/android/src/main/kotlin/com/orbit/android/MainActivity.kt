package com.orbit.android

import android.os.Bundle
import android.os.Build
import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.remember
import com.orbit.client.app.OrbitApp
import com.orbit.client.app.ShellNavigation
import kotlinx.coroutines.CompletableDeferred

class MainActivity : ComponentActivity() {
    private var permissionResult: CompletableDeferred<Boolean>? = null
    private val localNetworkPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        permissionResult?.complete(granted)
        permissionResult = null
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val app = application as OrbitApplication
        setContent {
            val navigation = remember { ShellNavigation() }
            BackHandler(enabled = navigation.settingsOpen || navigation.contactsOpen) { navigation.back() }
            OrbitApp(app.controller, app.preferences, navigation,
                prepareLocalNetwork = if (Build.VERSION.SDK_INT >= 37) ({ requestLocalNetwork() }) else null)
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

    override fun onDestroy() {
        permissionResult?.cancel()
        permissionResult = null
        super.onDestroy()
    }
}
