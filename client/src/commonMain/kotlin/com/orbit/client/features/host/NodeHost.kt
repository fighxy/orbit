package com.orbit.client.features.host

/** A node this computer is hosting, in the form other clients paste into Contacts. */
data class HostedNode(val address: String, val registrationCode: String)

class NodeHostException(message: String) : RuntimeException(message)

/** Starts the local node and returns the address and registration code. */
interface NodeHost {
    suspend fun becomeNode(): HostedNode
}

/** Reads the card printed by `orbit-node host`. */
object HostCard {
    fun parse(text: String): HostedNode? {
        val address = value(text, "node address:") ?: return null
        val code = value(text, "registration code:") ?: return null
        if ('@' !in address || code.length < 12 || code.any(Char::isWhitespace)) return null
        return HostedNode(address, code)
    }

    private fun value(text: String, prefix: String): String? =
        text.lineSequence().firstNotNullOfOrNull { line ->
            val trimmed = line.trim()
            if (trimmed.startsWith(prefix, ignoreCase = true)) {
                trimmed.substring(prefix.length).trim().takeIf { it.isNotEmpty() }
            } else {
                null
            }
        }
}
