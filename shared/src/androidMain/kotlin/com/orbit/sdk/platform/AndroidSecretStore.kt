package com.orbit.sdk.platform

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Secrets encrypted with a non-exportable AES-256-GCM key in the Android
 * Keystore (StrongBox when available) and stored in `noBackupFilesDir`, so
 * they are neither backed up nor migrated to other devices.
 *
 * Unreadable or undecryptable entries raise [SecureStorageException]; they are
 * never treated as absent, so a broken store cannot silently create a new
 * identity.
 */
class AndroidSecretStore(context: Context) : SecretStore {
    private val directory = File(context.noBackupFilesDir, "orbit-secrets")

    override suspend fun read(key: String): ByteArray? = withContext(Dispatchers.IO) {
        val file = fileFor(key)
        if (!file.exists()) return@withContext null
        guard("read") {
            val blob = file.readBytes()
            if (blob.size < HEADER_SIZE + TAG_BYTES || blob[0] != FORMAT_VERSION) {
                throw SecureStorageException("stored secret has an unknown format")
            }
            val iv = blob.copyOfRange(1, HEADER_SIZE)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            val secretKey = existingKey()
                ?: throw SecureStorageException("Keystore key is missing; the stored secret cannot be decrypted")
            cipher.init(Cipher.DECRYPT_MODE, secretKey, GCMParameterSpec(TAG_BYTES * 8, iv))
            cipher.updateAAD(aad(key))
            cipher.doFinal(blob, HEADER_SIZE, blob.size - HEADER_SIZE)
        }
    }

    override suspend fun write(key: String, value: ByteArray) = withContext(Dispatchers.IO) {
        guard("write") {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, existingKey() ?: generateKey())
            cipher.updateAAD(aad(key))
            val ciphertext = cipher.doFinal(value)
            val iv = cipher.iv
            check(iv.size == IV_BYTES) { "unexpected IV size" }
            directory.mkdirs()
            val target = fileFor(key)
            val temp = File(directory, "${target.name}.tmp")
            temp.outputStream().use { out ->
                out.write(byteArrayOf(FORMAT_VERSION))
                out.write(iv)
                out.write(ciphertext)
                out.fd.sync()
            }
            if (!temp.renameTo(target)) throw IOException("cannot replace ${target.name}")
        }
    }

    override suspend fun delete(key: String) = withContext(Dispatchers.IO) {
        val file = fileFor(key)
        if (file.exists() && !file.delete()) throw SecureStorageException("cannot delete stored secret")
    }

    private fun fileFor(key: String): File =
        File(directory, key.encodeToByteArray().joinToString("") { "%02x".format(it) } + ".bin")

    private fun aad(key: String): ByteArray = "orbit/android-secret/v1/$key".encodeToByteArray()

    private fun existingKey(): SecretKey? {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        return keyStore.getKey(KEY_ALIAS, null) as SecretKey?
    }

    private fun generateKey(): SecretKey {
        fun spec(strongBox: Boolean) = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setRandomizedEncryptionRequired(true)
            .apply { if (strongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) setIsStrongBoxBacked(true) }
            .build()

        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        return try {
            generator.init(spec(strongBox = true))
            generator.generateKey()
        } catch (e: StrongBoxUnavailableException) {
            // Still a hardware-backed (TEE) Keystore key, not a software fallback.
            generator.init(spec(strongBox = false))
            generator.generateKey()
        }
    }

    private inline fun <T> guard(operation: String, block: () -> T): T = try {
        block()
    } catch (e: SecureStorageException) {
        throw e
    } catch (e: GeneralSecurityException) {
        throw SecureStorageException("Android Keystore $operation failed: ${e.javaClass.simpleName}", e)
    } catch (e: IOException) {
        throw SecureStorageException("secret $operation failed: ${e.message}", e)
    } catch (e: IllegalStateException) {
        throw SecureStorageException("secret $operation failed: ${e.message}", e)
    }

    private companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val KEY_ALIAS = "orbit.secret-store.v1"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val FORMAT_VERSION: Byte = 1
        const val IV_BYTES = 12
        const val TAG_BYTES = 16
        const val HEADER_SIZE = 1 + IV_BYTES
    }
}
