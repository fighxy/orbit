package com.orbit.shared

/**
 * Entry point for the shared Orbit core.
 *
 * This module holds all platform-independent logic: identity, rooms (Autobase),
 * messaging, media (Hyperdrive), and offline delivery (blind-peering).
 *
 * Platform-specific implementations (local DB, keychain, push) live in
 * androidMain / iosMain source sets and are wired through expect/actual.
 */
object Orbit {
    const val VERSION: String = "0.1.0"

    fun greet(): String = "Orbit $VERSION — P2P messenger core"
}
