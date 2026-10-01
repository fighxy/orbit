package com.orbit.sdk.platform

/**
 * Deletes the engine data directory and everything under it.
 *
 * Call only after the engine is closed. While it is open it holds
 * `accounts/<hex>/engine.lock`, and a partial wipe would leave ciphertext
 * beside the next account.
 */
expect fun deleteDataDir(path: String)
