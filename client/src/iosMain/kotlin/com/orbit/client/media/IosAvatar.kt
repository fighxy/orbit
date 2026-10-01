@file:OptIn(ExperimentalForeignApi::class)

package com.orbit.client.media

import com.orbit.client.designsystem.Strings
import com.orbit.client.features.settings.AvatarPick
import kotlin.coroutines.resume
import kotlin.io.encoding.Base64
import kotlinx.coroutines.CancellableContinuation
import kotlin.math.min
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.useContents
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import platform.CoreGraphics.CGRectMake
import platform.CoreGraphics.CGSizeMake
import platform.Foundation.NSData
import platform.Foundation.NSItemProvider
import platform.PhotosUI.PHPickerConfiguration
import platform.PhotosUI.PHPickerFilter
import platform.PhotosUI.PHPickerResult
import platform.PhotosUI.PHPickerViewController
import platform.PhotosUI.PHPickerViewControllerDelegateProtocol
import platform.UIKit.UIColor
import platform.UIKit.UIGraphicsBeginImageContextWithOptions
import platform.UIKit.UIGraphicsEndImageContext
import platform.UIKit.UIGraphicsGetImageFromCurrentImageContext
import platform.UIKit.UIImage
import platform.UIKit.UIImageJPEGRepresentation
import platform.UIKit.UIRectFill
import platform.UIKit.UIViewController
import platform.darwin.NSObject

private const val MAX_SOURCE_BYTES = 25L * 1024 * 1024
private const val MAX_PIXELS = 24_000_000.0
private const val MAX_AVATAR_BYTES = 32 * 1024

/**
 * PHPicker runs out of process, so the photo library usage string is not required.
 * The delegate is retained here: UIKit keeps it only weakly.
 */
class IosAvatarPicker(
    private val presenter: () -> UIViewController,
) {
    private val main = CoroutineScope(Dispatchers.Main)
    private var delegate: NSObject? = null

    suspend fun pick(): AvatarPick = suspendCancellableCoroutine { cont ->
        val pickerDelegate = object : NSObject(), PHPickerViewControllerDelegateProtocol {
            override fun picker(picker: PHPickerViewController, didFinishPicking: List<*>) {
                picker.dismissViewControllerAnimated(flag = true, completion = null)
                delegate = null
                val provider = (didFinishPicking.firstOrNull() as? PHPickerResult)?.itemProvider
                if (provider == null) {
                    if (cont.isActive) cont.resume(AvatarPick.Cancelled)
                    return
                }
                load(provider, cont)
            }
        }
        delegate = pickerDelegate
        cont.invokeOnCancellation {
            delegate = null
            presenter().dismissViewControllerAnimated(flag = true, completion = null)
        }
        val configuration = PHPickerConfiguration()
        configuration.selectionLimit = 1L
        configuration.filter = PHPickerFilter.imagesFilter
        val picker = PHPickerViewController(configuration = configuration)
        picker.delegate = pickerDelegate
        presenter().presentViewController(picker, animated = true, completion = null)
    }

    private fun load(
        provider: NSItemProvider,
        cont: CancellableContinuation<AvatarPick>,
    ) {
        if (!provider.hasItemConformingToTypeIdentifier("public.image")) {
            if (cont.isActive) cont.resume(AvatarPick.Rejected(Strings.avatarUnreadable))
            return
        }
        provider.loadDataRepresentationForTypeIdentifier("public.image") { data, _ ->
            if (!cont.isActive) return@loadDataRepresentationForTypeIdentifier
            if (data == null || data.length > MAX_SOURCE_BYTES.toULong()) {
                main.launch { if (cont.isActive) cont.resume(AvatarPick.Rejected(Strings.avatarUnreadable)) }
                return@loadDataRepresentationForTypeIdentifier
            }
            main.launch {
                if (!cont.isActive) return@launch
                cont.resume(encode(data))
            }
        }
    }

    private fun encode(data: NSData): AvatarPick {
        val image = UIImage.imageWithData(data) ?: return AvatarPick.Rejected(Strings.avatarUnreadable)
        val srcW = image.size.useContents { width }
        val srcH = image.size.useContents { height }
        if (srcW < 1.0 || srcH < 1.0 || srcW * srcH > MAX_PIXELS) {
            return AvatarPick.Rejected(Strings.avatarUnreadable)
        }
        var edge = min(min(srcW, srcH), 256.0).toInt()
        while (edge > 0) {
            val square = drawSquare(image, srcW, srcH, edge.toDouble())
                ?: return AvatarPick.Rejected(Strings.avatarUnreadable)
            var quality = 85
            while (quality >= 40) {
                val jpeg = UIImageJPEGRepresentation(square, quality / 100.0)?.toByteArray()
                if (jpeg == null) return AvatarPick.Rejected(Strings.avatarUnreadable)
                if (jpeg.size <= MAX_AVATAR_BYTES &&
                    jpeg.size >= 3 &&
                    jpeg[0] == 0xFF.toByte() &&
                    jpeg[1] == 0xD8.toByte() &&
                    jpeg[2] == 0xFF.toByte()
                ) {
                    return AvatarPick.Image(Base64.Default.encode(jpeg))
                }
                quality -= 15
            }
            val next = edge * 3 / 4
            if (next < 64 || next >= edge) break
            edge = next
        }
        return AvatarPick.Rejected(Strings.avatarTooLarge)
    }

    private fun drawSquare(image: UIImage, srcW: Double, srcH: Double, edge: Double): UIImage? {
        UIGraphicsBeginImageContextWithOptions(CGSizeMake(edge, edge), true, 1.0)
        return try {
            UIColor.whiteColor.setFill()
            UIRectFill(CGRectMake(0.0, 0.0, edge, edge))
            val side = min(srcW, srcH)
            val scale = edge / side
            val drawW = srcW * scale
            val drawH = srcH * scale
            image.drawInRect(CGRectMake((edge - drawW) / 2.0, (edge - drawH) / 2.0, drawW, drawH))
            UIGraphicsGetImageFromCurrentImageContext()
        } finally {
            UIGraphicsEndImageContext()
        }
    }
}
