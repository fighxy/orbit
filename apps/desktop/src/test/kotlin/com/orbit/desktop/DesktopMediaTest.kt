package com.orbit.desktop

import com.orbit.client.features.settings.AvatarPick
import java.awt.image.BufferedImage
import java.io.ByteArrayInputStream
import javax.imageio.ImageIO
import javax.sound.sampled.AudioFormat
import kotlin.io.encoding.Base64
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DesktopMediaTest {
    @Test
    fun stereo48kFoldsToMono16k() {
        val pcm = ByteArray(12)
        pcm[0] = 0
        pcm[1] = 0
        pcm[4] = 10
        pcm[5] = 0
        pcm[8] = 20
        pcm[9] = 0
        val folded = toMono16k(pcm, PcmLayout(48_000, 2))
        assertEquals(2, folded.size)
        assertEquals(10, folded[0].toInt() and 0xFF)
        assertEquals(0, folded[1].toInt() and 0xFF)
    }

    @Test
    fun speakerDuplicateFillsBothChannels() {
        val pcm = byteArrayOf(1, 0, 2, 0)
        val stereo = toSpeaker(pcm, PcmLayout(16_000, 2))
        assertEquals(byteArrayOf(1, 0, 1, 0, 2, 0, 2, 0).toList(), stereo.toList())
    }

    @Test
    fun captureFormatRejectsFloatAndOddRates() {
        assertEquals(PcmLayout(48_000, 1), matchesVoiceCapture(AudioFormat(48_000f, 16, 1, true, false)))
        assertNull(matchesVoiceCapture(AudioFormat(44_100f, 16, 1, true, false)))
        assertNull(matchesVoiceCapture(AudioFormat(16_000f, 16, 1, true, true)))
    }

    @Test
    fun jpegOrientationReadsTheShortTag() {
        val app = byteArrayOf(
            0xFF.toByte(), 0xD8.toByte(),
            0xFF.toByte(), 0xE1.toByte(),
            0x00, 0x22,
            'E'.code.toByte(), 'x'.code.toByte(), 'i'.code.toByte(), 'f'.code.toByte(), 0, 0,
            'I'.code.toByte(), 'I'.code.toByte(),
            42, 0,
            8, 0, 0, 0,
            1, 0,
            0x12, 0x01,
            3, 0,
            1, 0, 0, 0,
            6, 0, 0, 0,
            0, 0, 0, 0,
            0xFF.toByte(), 0xD9.toByte(),
        )
        assertEquals(6, jpegOrientation(app))
        assertEquals(1, jpegOrientation(byteArrayOf(0x89.toByte(), 0x50)))
    }

    @Test
    fun rotate90ClockwiseTurnsTheLeftPixelIntoTheTopPixel() {
        val source = BufferedImage(2, 1, BufferedImage.TYPE_INT_RGB)
        source.setRGB(0, 0, 0xFF0000)
        source.setRGB(1, 0, 0x0000FF)
        val turned = upright(source, 6)
        assertEquals(1, turned.width)
        assertEquals(2, turned.height)
        assertEquals(0xFF0000, turned.getRGB(0, 0) and 0xFFFFFF)
        assertEquals(0x0000FF, turned.getRGB(0, 1) and 0xFFFFFF)
    }

    @Test
    fun scaledAvatarIsASmallSquareJpeg() {
        val source = BufferedImage(80, 30, BufferedImage.TYPE_INT_RGB)
        for (x in 0 until source.width) {
            for (y in 0 until source.height) {
                source.setRGB(x, y, (x * 32) shl 16 or (y * 64) shl 8 or 0x40)
            }
        }
        val pick = scaleAvatarImage(source, 1)
        val image = assertIs<AvatarPick.Image>(pick)
        val jpeg = Base64.Default.decode(image.base64)
        assertTrue(jpeg.size in 3..(32 * 1024))
        assertEquals(0xFF.toByte(), jpeg[0])
        assertEquals(0xD8.toByte(), jpeg[1])
        val decoded = ImageIO.read(ByteArrayInputStream(jpeg))
        assertEquals(decoded.width, decoded.height)
        assertTrue(decoded.width <= 256)
    }
}
