package com.orbit.sdk.platform

import java.io.File

/** Per-user application data locations on desktop operating systems. */
object DesktopPaths {
    /**
     * Data directory for [profile]:
     * - Windows: `%LOCALAPPDATA%\Orbit\<profile>`; local rather than roaming,
     *   because the data is bound to this device's key
     * - macOS: `~/Library/Application Support/Orbit/<profile>`
     * - Linux and others: `$XDG_DATA_HOME/orbit/<profile>` or `~/.local/share/orbit/<profile>`
     */
    fun dataDir(profile: String): File {
        require(PROFILE_NAME.matches(profile)) { "profile must match ${PROFILE_NAME.pattern}" }
        val os = System.getProperty("os.name").lowercase()
        val home = System.getProperty("user.home")
        val base = when {
            os.contains("win") -> File(
                System.getenv("LOCALAPPDATA")?.takeIf { it.isNotBlank() }
                    ?: throw IllegalStateException("LOCALAPPDATA is not set"),
                "Orbit",
            )
            os.contains("mac") -> File(home, "Library/Application Support/Orbit")
            else -> File(System.getenv("XDG_DATA_HOME")?.takeIf { it.isNotBlank() } ?: "$home/.local/share", "orbit")
        }
        return File(base, profile).absoluteFile
    }

    private val PROFILE_NAME = Regex("[a-z0-9][a-z0-9_-]{0,31}")
}
