package org.weirdbrains.morph

import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class QrDecodeTest {
    /** A grey camera-like frame with a QR code drawn into it, row stride wider than the image. */
    private fun frame(text: String, width: Int, height: Int, stride: Int): ByteArray {
        val size = minOf(width, height) * 3 / 4
        val matrix = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, size, size, mapOf(EncodeHintType.MARGIN to 2))
        val luma = ByteArray(stride * height) { 0x80.toByte() }
        val left = (width - size) / 2
        val top = (height - size) / 2
        for (y in 0 until size) {
            for (x in 0 until size) {
                luma[(top + y) * stride + left + x] = if (matrix[x, y]) 0x10 else 0xf0.toByte()
            }
        }
        return luma
    }

    @Test
    fun readsAMorphLinkFromAFrame() {
        val link = "weirdnet-morph://link/" + "A".repeat(340)
        val decoded = QrDecode().decode(frame(link, 640, 480, 704), 704, 640, 480)
        assertEquals(link, decoded)
    }

    @Test
    fun aFrameWithoutACodeIsNull() {
        val blank = ByteArray(704 * 480) { 0x80.toByte() }
        assertNull(QrDecode().decode(blank, 704, 640, 480))
    }
}
