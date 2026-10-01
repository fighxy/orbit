@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class)

package com.orbit.sdk.platform

import platform.Foundation.NSFileManager

actual fun deleteDataDir(path: String) {
    val manager = NSFileManager.defaultManager
    if (!manager.fileExistsAtPath(path)) return
    if (!manager.removeItemAtPath(path, error = null)) {
        throw SecureStorageException("failed to delete data directory")
    }
}
