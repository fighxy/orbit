package com.orbit.sdk.platform

import android.content.Context
import java.io.File

object AndroidPaths {
    /**
     * Engine data directory. Excluded from backup: its contents are encrypted
     * with a device key that never leaves this device's Keystore.
     */
    fun dataDir(context: Context): File = File(context.noBackupFilesDir, "orbit").absoluteFile
}
