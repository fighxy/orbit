package com.orbit.sdk.platform

import java.io.File

actual fun deleteDataDir(path: String) {
    val root = File(path)
    if (!root.exists()) return
    if (!root.deleteRecursively()) {
        throw SecureStorageException("failed to delete data directory")
    }
}
