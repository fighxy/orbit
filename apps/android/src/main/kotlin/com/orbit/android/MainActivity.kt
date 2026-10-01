package com.orbit.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.remember
import com.orbit.client.app.OrbitApp
import com.orbit.client.app.ShellNavigation

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val app = application as OrbitApplication
        setContent {
            val navigation = remember { ShellNavigation() }
            BackHandler(enabled = navigation.settingsOpen || navigation.contactsOpen) { navigation.back() }
            OrbitApp(app.controller, app.preferences, navigation)
        }
    }
}
