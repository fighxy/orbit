@file:OptIn(ExperimentalForeignApi::class)

package com.orbit.sdk.platform

import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.alloc
import kotlinx.cinterop.convert
import kotlinx.cinterop.memScoped
import kotlinx.cinterop.ptr
import kotlinx.cinterop.usePinned
import kotlinx.cinterop.value
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.withContext
import platform.CoreFoundation.CFDictionaryAddValue
import platform.CoreFoundation.CFDictionaryCreateMutable
import platform.CoreFoundation.CFMutableDictionaryRef
import platform.CoreFoundation.CFRelease
import platform.CoreFoundation.CFTypeRef
import platform.CoreFoundation.CFTypeRefVar
import platform.CoreFoundation.kCFBooleanTrue
import platform.CoreFoundation.kCFTypeDictionaryKeyCallBacks
import platform.CoreFoundation.kCFTypeDictionaryValueCallBacks
import platform.Foundation.CFBridgingRelease
import platform.Foundation.CFBridgingRetain
import platform.Foundation.NSData
import platform.Foundation.create
import platform.Security.SecItemAdd
import platform.Security.SecItemCopyMatching
import platform.Security.SecItemDelete
import platform.Security.SecItemUpdate
import platform.Security.errSecDuplicateItem
import platform.Security.errSecItemNotFound
import platform.Security.errSecSuccess
import platform.Security.kSecAttrAccessible
import platform.Security.kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
import platform.Security.kSecAttrAccount
import platform.Security.kSecAttrService
import platform.Security.kSecClass
import platform.Security.kSecClassGenericPassword
import platform.Security.kSecMatchLimit
import platform.Security.kSecMatchLimitOne
import platform.Security.kSecReturnData
import platform.Security.kSecValueData
import platform.posix.memcpy

/**
 * Generic-password Keychain items, readable after the first unlock and never
 * synchronized or migrated to another device (`ThisDeviceOnly`).
 */
class KeychainSecretStore(private val service: String = DEFAULT_SERVICE) : SecretStore {
    override suspend fun read(key: String): ByteArray? = withContext(Dispatchers.IO) {
        memScoped {
            val query = query(key) {
                add(kSecReturnData, kCFBooleanTrue)
                add(kSecMatchLimit, kSecMatchLimitOne)
            }
            val result = alloc<CFTypeRefVar>()
            val status = try {
                SecItemCopyMatching(query, result.ptr)
            } finally {
                CFRelease(query)
            }
            when (status) {
                errSecSuccess -> (CFBridgingRelease(result.value) as NSData).toByteArray()
                errSecItemNotFound -> null
                else -> throw SecureStorageException("Keychain read failed with OSStatus $status")
            }
        }
    }

    override suspend fun write(key: String, value: ByteArray) = withContext(Dispatchers.IO) {
        require(value.isNotEmpty()) { "secret must not be empty" }
        val data = value.toNSData()
        val add = query(key) {
            addRetained(kSecValueData, data)
            add(kSecAttrAccessible, kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly)
        }
        val status = try {
            SecItemAdd(add, null)
        } finally {
            CFRelease(add)
        }
        val finalStatus = if (status == errSecDuplicateItem) {
            val match = query(key) {}
            val update = dictionary { addRetained(kSecValueData, data) }
            try {
                SecItemUpdate(match, update)
            } finally {
                CFRelease(match)
                CFRelease(update)
            }
        } else {
            status
        }
        if (finalStatus != errSecSuccess) throw SecureStorageException("Keychain write failed with OSStatus $finalStatus")
    }

    override suspend fun delete(key: String) = withContext(Dispatchers.IO) {
        val match = query(key) {}
        val status = try {
            SecItemDelete(match)
        } finally {
            CFRelease(match)
        }
        if (status != errSecSuccess && status != errSecItemNotFound) {
            throw SecureStorageException("Keychain delete failed with OSStatus $status")
        }
    }

    private fun query(key: String, extra: DictionaryBuilder.() -> Unit): CFMutableDictionaryRef = dictionary {
        add(kSecClass, kSecClassGenericPassword)
        addRetained(kSecAttrService, service)
        addRetained(kSecAttrAccount, key)
        extra()
    }

    companion object {
        const val DEFAULT_SERVICE: String = "com.orbit.messenger"
    }
}

private class DictionaryBuilder(val dictionary: CFMutableDictionaryRef) {
    fun add(key: CFTypeRef?, value: CFTypeRef?) = CFDictionaryAddValue(dictionary, key, value)

    /** Bridges a Kotlin/Objective-C object; the dictionary keeps its own reference. */
    fun addRetained(key: CFTypeRef?, value: Any) {
        val retained = CFBridgingRetain(value)
        try {
            CFDictionaryAddValue(dictionary, key, retained)
        } finally {
            CFRelease(retained)
        }
    }
}

private fun dictionary(build: DictionaryBuilder.() -> Unit): CFMutableDictionaryRef {
    val dictionary = checkNotNull(
        CFDictionaryCreateMutable(null, 0, kCFTypeDictionaryKeyCallBacks.ptr, kCFTypeDictionaryValueCallBacks.ptr),
    ) { "cannot allocate a CFDictionary" }
    DictionaryBuilder(dictionary).build()
    return dictionary
}

private fun ByteArray.toNSData(): NSData = usePinned { pinned ->
    NSData.create(bytes = pinned.addressOf(0), length = size.convert())
}

private fun NSData.toByteArray(): ByteArray {
    val size = length.toInt()
    val bytes = ByteArray(size)
    if (size > 0) bytes.usePinned { pinned -> memcpy(pinned.addressOf(0), this.bytes, length) }
    return bytes
}
