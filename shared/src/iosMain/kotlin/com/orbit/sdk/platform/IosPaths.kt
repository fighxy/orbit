@file:OptIn(ExperimentalForeignApi::class)

package com.orbit.sdk.platform

import kotlinx.cinterop.ExperimentalForeignApi
import platform.Foundation.NSApplicationSupportDirectory
import platform.Foundation.NSFileManager
import platform.Foundation.NSUserDomainMask

object IosPaths {
    /** `Application Support/Orbit` inside the app container. */
    fun dataDir(): String {
        val base = NSFileManager.defaultManager.URLForDirectory(
            directory = NSApplicationSupportDirectory,
            inDomain = NSUserDomainMask,
            appropriateForURL = null,
            create = true,
            error = null,
        )
        val path = checkNotNull(base?.path) { "Application Support directory is unavailable" }
        return "$path/Orbit"
    }
}
