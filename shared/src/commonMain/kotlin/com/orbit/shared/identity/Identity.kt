package com.orbit.shared.identity

/**
 * User identity derived from a 24-word seed phrase.
 *
 * Ed25519 keypairs are generated hierarchically via keet-identity-key
 * (JS layer, bridged through expect/actual). No phone number, no email.
 */
data class Identity(
    val publicKey: ByteArray,
    val deviceKey: ByteArray,
    val seedPhrase: List<String>
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is Identity) return false
        return publicKey.contentEquals(other.publicKey) &&
            deviceKey.contentEquals(other.deviceKey) &&
            seedPhrase == other.seedPhrase
    }

    override fun hashCode(): Int {
        var result = publicKey.contentHashCode()
        result = 31 * result + deviceKey.contentHashCode()
        result = 31 * result + seedPhrase.hashCode()
        return result
    }
}
