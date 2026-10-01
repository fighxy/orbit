package com.orbit.android

import android.content.ContentResolver
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.net.Uri
import androidx.exifinterface.media.ExifInterface
import com.orbit.client.designsystem.Strings
import com.orbit.client.features.settings.AvatarPick
import java.io.ByteArrayOutputStream
import kotlin.io.encoding.Base64
import kotlin.math.max
import kotlin.math.min

/** Center-crops a gallery image into a JPEG the core accepts: at most 32 KiB. */
fun scaleAvatarJpeg(resolver: ContentResolver, uri: Uri): AvatarPick {
    val orientation = resolver.openInputStream(uri)?.use { stream ->
        ExifInterface(stream).getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_UNDEFINED)
    } ?: return AvatarPick.Rejected(Strings.avatarUnreadable)
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
        ?: return AvatarPick.Rejected(Strings.avatarUnreadable)
    if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return AvatarPick.Rejected(Strings.avatarUnreadable)
    var sample = 1
    val edge = max(bounds.outWidth, bounds.outHeight)
    while (edge / sample > 1_280) sample *= 2
    val decoded = resolver.openInputStream(uri)?.use {
        BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample })
    } ?: return AvatarPick.Rejected(Strings.avatarUnreadable)
    val upright = rotate(decoded, orientation)
    val side = min(upright.width, upright.height)
    val square = Bitmap.createBitmap(upright, (upright.width - side) / 2, (upright.height - side) / 2, side, side)
    val sized = if (side == 256) square else Bitmap.createScaledBitmap(square, 256.coerceAtMost(side), 256.coerceAtMost(side), true)
    val jpeg = compressUnderCap(sized) ?: return AvatarPick.Rejected(Strings.avatarTooLarge)
    if (jpeg.size < 3 || jpeg[0] != 0xFF.toByte() || jpeg[1] != 0xD8.toByte()) {
        return AvatarPick.Rejected(Strings.avatarUnreadable)
    }
    return AvatarPick.Image(Base64.Default.encode(jpeg))
}

private fun rotate(source: Bitmap, orientation: Int): Bitmap {
    val matrix = Matrix()
    when (orientation) {
        ExifInterface.ORIENTATION_ROTATE_90 -> matrix.postRotate(90f)
        ExifInterface.ORIENTATION_ROTATE_180 -> matrix.postRotate(180f)
        ExifInterface.ORIENTATION_ROTATE_270 -> matrix.postRotate(270f)
        ExifInterface.ORIENTATION_FLIP_HORIZONTAL -> matrix.postScale(-1f, 1f)
        ExifInterface.ORIENTATION_FLIP_VERTICAL -> matrix.postScale(1f, -1f)
        ExifInterface.ORIENTATION_TRANSPOSE -> {
            matrix.postRotate(90f)
            matrix.postScale(-1f, 1f)
        }
        ExifInterface.ORIENTATION_TRANSVERSE -> {
            matrix.postRotate(270f)
            matrix.postScale(-1f, 1f)
        }
        else -> return source
    }
    return Bitmap.createBitmap(source, 0, 0, source.width, source.height, matrix, true)
}

private fun compressUnderCap(source: Bitmap): ByteArray? {
    var current = source
    var edge = current.width
    while (edge > 0) {
        var quality = 85
        while (quality >= 40) {
            val stream = ByteArrayOutputStream()
            if (!current.compress(Bitmap.CompressFormat.JPEG, quality, stream)) return null
            val bytes = stream.toByteArray()
            if (bytes.size <= 32 * 1024) return bytes
            quality -= 15
        }
        val next = edge * 3 / 4
        if (next < 64 || next >= edge) break
        current = Bitmap.createScaledBitmap(current, next, next, true)
        edge = next
    }
    return null
}
