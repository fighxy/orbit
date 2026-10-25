package com.orbit.desktop

import com.orbit.client.designsystem.Strings
import com.orbit.client.features.settings.AvatarPick
import java.awt.Color
import java.awt.Frame
import java.awt.FileDialog
import java.awt.RenderingHints
import java.awt.geom.AffineTransform
import java.awt.image.BufferedImage
import java.io.ByteArrayOutputStream
import java.io.File
import javax.imageio.IIOImage
import javax.imageio.ImageIO
import javax.imageio.ImageWriteParam
import kotlin.coroutines.cancellation.CancellationException
import kotlin.io.encoding.Base64
import kotlin.math.min
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Opens a file dialog and returns a JPEG the core will store, or a cancellation. */
internal suspend fun pickDesktopAvatar(owner: Frame): AvatarPick = withContext(Dispatchers.Main.immediate) {
    val dialog = FileDialog(owner, Strings.choosePhoto, FileDialog.LOAD)
    dialog.isVisible = true
    val name = dialog.file ?: return@withContext AvatarPick.Cancelled
    val directory = dialog.directory ?: return@withContext AvatarPick.Cancelled
    withContext(Dispatchers.Default) {
        try {
            scaleAvatarFile(File(directory, name))
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            AvatarPick.Rejected(Strings.avatarUnreadable)
        }
    }
}

internal fun scaleAvatarFile(file: File): AvatarPick {
    if (!file.isFile || file.length() <= 0L || file.length() > 25L * 1024 * 1024) {
        return AvatarPick.Rejected(Strings.avatarUnreadable)
    }
    val bytes = file.readBytes()
    val image = ImageIO.read(bytes.inputStream()) ?: return AvatarPick.Rejected(Strings.avatarUnreadable)
    if (image.width <= 0 || image.height <= 0 || image.width.toLong() * image.height > 24_000_000L) {
        return AvatarPick.Rejected(Strings.avatarUnreadable)
    }
    return scaleAvatarImage(image, jpegOrientation(bytes))
}

internal fun scaleAvatarImage(source: BufferedImage, orientation: Int): AvatarPick {
    val turned = upright(source, orientation)
    val side = min(turned.width, turned.height)
    val square = turned.getSubimage((turned.width - side) / 2, (turned.height - side) / 2, side, side)
    var current = scaleSquare(square, min(side, 256))
    var edge = current.width
    while (edge > 0) {
        var quality = 0.85f
        while (quality >= 0.40f - 0.001f) {
            val jpeg = jpegBytes(current, quality)
            if (jpeg != null && jpeg.size <= 32 * 1024 && jpeg.size >= 3 &&
                jpeg[0] == 0xFF.toByte() && jpeg[1] == 0xD8.toByte()
            ) {
                return AvatarPick.Image(Base64.Default.encode(jpeg))
            }
            quality -= 0.15f
        }
        val next = edge * 3 / 4
        if (next < 64 || next >= edge) break
        current = scaleSquare(current, next)
        edge = next
    }
    return AvatarPick.Rejected(Strings.avatarTooLarge)
}

/** EXIF orientation 1..8, or 1 when the file has no readable tag. */
internal fun jpegOrientation(bytes: ByteArray): Int {
    if (bytes.size < 4 || bytes[0] != 0xFF.toByte() || bytes[1] != 0xD8.toByte()) return 1
    var index = 2
    while (index + 4 < bytes.size) {
        if (bytes[index] != 0xFF.toByte()) return 1
        var marker = bytes[index + 1].toInt() and 0xFF
        while (marker == 0xFF && index + 2 < bytes.size) {
            index += 1
            marker = bytes[index + 1].toInt() and 0xFF
        }
        index += 2
        if (marker == 0xD8 || marker == 0xD9 || marker == 0xDA || marker == 0x01 || marker in 0xD0..0xD7) {
            if (marker == 0xD9 || marker == 0xDA) return 1
            continue
        }
        if (index + 2 > bytes.size) return 1
        val length = ((bytes[index].toInt() and 0xFF) shl 8) or (bytes[index + 1].toInt() and 0xFF)
        if (length < 2 || index + length > bytes.size) return 1
        if (marker == 0xE1) {
            exifOrientation(bytes, index + 2, index + length)?.let { return it }
        }
        index += length
    }
    return 1
}

internal fun upright(source: BufferedImage, orientation: Int): BufferedImage {
    val width = source.width
    val height = source.height
    val swap = orientation == 5 || orientation == 6 || orientation == 7 || orientation == 8
    val out = BufferedImage(if (swap) height else width, if (swap) width else height, BufferedImage.TYPE_INT_RGB)
    val graphics = out.createGraphics()
    try {
        graphics.color = Color.WHITE
        graphics.fillRect(0, 0, out.width, out.height)
        graphics.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_NEAREST_NEIGHBOR)
        val transform = AffineTransform()
        when (orientation) {
            2 -> {
                transform.translate(width.toDouble(), 0.0)
                transform.scale(-1.0, 1.0)
            }
            3 -> transform.rotate(Math.PI, width / 2.0, height / 2.0)
            4 -> {
                transform.translate(0.0, height.toDouble())
                transform.scale(1.0, -1.0)
            }
            6 -> {
                transform.translate(height.toDouble(), 0.0)
                transform.rotate(Math.PI / 2)
            }
            8 -> {
                transform.translate(0.0, width.toDouble())
                transform.rotate(-Math.PI / 2)
            }
            5 -> {
                transform.translate(height.toDouble(), 0.0)
                transform.rotate(Math.PI / 2)
                transform.translate(width.toDouble(), 0.0)
                transform.scale(-1.0, 1.0)
            }
            7 -> {
                transform.translate(0.0, width.toDouble())
                transform.rotate(-Math.PI / 2)
                transform.translate(width.toDouble(), 0.0)
                transform.scale(-1.0, 1.0)
            }
            else -> Unit
        }
        graphics.transform(transform)
        graphics.drawImage(source, 0, 0, null)
    } finally {
        graphics.dispose()
    }
    return out
}

private fun scaleSquare(source: BufferedImage, edge: Int): BufferedImage {
    val out = BufferedImage(edge, edge, BufferedImage.TYPE_INT_RGB)
    val graphics = out.createGraphics()
    try {
        graphics.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_BILINEAR)
        graphics.drawImage(source, 0, 0, edge, edge, null)
    } finally {
        graphics.dispose()
    }
    return out
}

private fun jpegBytes(image: BufferedImage, quality: Float): ByteArray? {
    val writers = ImageIO.getImageWritersByFormatName("jpeg")
    if (!writers.hasNext()) return null
    val writer = writers.next()
    val stream = ByteArrayOutputStream()
    return try {
        ImageIO.createImageOutputStream(stream).use { output ->
            writer.output = output
            val param = writer.defaultWriteParam
            if (param.canWriteCompressed()) {
                param.compressionMode = ImageWriteParam.MODE_EXPLICIT
                param.compressionQuality = quality
            }
            writer.write(null, IIOImage(image, null, null), param)
        }
        stream.toByteArray()
    } finally {
        writer.dispose()
    }
}

private fun exifOrientation(bytes: ByteArray, start: Int, end: Int): Int? {
    if (end - start < 8) return null
    if (bytes[start] != 'E'.code.toByte() || bytes[start + 1] != 'x'.code.toByte() ||
        bytes[start + 2] != 'i'.code.toByte() || bytes[start + 3] != 'f'.code.toByte() ||
        bytes[start + 4] != 0.toByte() || bytes[start + 5] != 0.toByte()
    ) {
        return null
    }
    return tiffOrientation(bytes, start + 6, end)
}

private fun tiffOrientation(bytes: ByteArray, start: Int, end: Int): Int? {
    if (end - start < 8) return null
    val little = when {
        bytes[start] == 'I'.code.toByte() && bytes[start + 1] == 'I'.code.toByte() -> true
        bytes[start] == 'M'.code.toByte() && bytes[start + 1] == 'M'.code.toByte() -> false
        else -> return null
    }
    if (u16(bytes, start + 2, little) != 42) return null
    val offset = u32(bytes, start + 4, little)
    if (offset < 0) return null
    var cursor = start + offset
    if (cursor < start || cursor + 2 > end) return null
    val count = u16(bytes, cursor, little)
    cursor += 2
    repeat(count) {
        if (cursor + 12 > end) return null
        val tag = u16(bytes, cursor, little)
        val type = u16(bytes, cursor + 2, little)
        if (tag == 0x0112 && type == 3) {
            val value = u16(bytes, cursor + 8, little)
            if (value in 1..8) return value
        }
        cursor += 12
    }
    return null
}

private fun u16(bytes: ByteArray, index: Int, little: Boolean): Int {
    val first = bytes[index].toInt() and 0xFF
    val second = bytes[index + 1].toInt() and 0xFF
    return if (little) first or (second shl 8) else (first shl 8) or second
}

private fun u32(bytes: ByteArray, index: Int, little: Boolean): Int {
    val a = bytes[index].toInt() and 0xFF
    val b = bytes[index + 1].toInt() and 0xFF
    val c = bytes[index + 2].toInt() and 0xFF
    val d = bytes[index + 3].toInt() and 0xFF
    return if (little) a or (b shl 8) or (c shl 16) or (d shl 24) else (a shl 24) or (b shl 16) or (c shl 8) or d
}
