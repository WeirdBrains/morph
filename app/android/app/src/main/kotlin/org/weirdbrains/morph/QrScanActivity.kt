package org.weirdbrains.morph

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Bundle
import android.view.Gravity
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * Full-screen camera that returns the first QR code it sees (CameraX for
 * the camera, ZXing for decoding; both FOSS). Asks for the camera only
 * when opened. Frames stay in memory and are dropped as soon as they are
 * decoded.
 */
class QrScanActivity : ComponentActivity() {
    companion object {
        const val EXTRA_TEXT = "text"
        const val EXTRA_ERROR = "error"
        private const val PERMISSION_REQUEST = 0x7170
    }

    private lateinit var preview: PreviewView
    private val analysis: ExecutorService = Executors.newSingleThreadExecutor()
    private val decoder = QrDecode()

    @Volatile
    private var done = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        preview = PreviewView(this)
        val hint = TextView(this).apply {
            text = "Point the camera at the code on the new device"
            setTextColor(Color.WHITE)
            setBackgroundColor(0x99000000.toInt())
            setPadding(32, 24, 32, 24)
            gravity = Gravity.CENTER
        }
        val cancel = Button(this).apply {
            text = "Cancel"
            setOnClickListener { finishWith(Activity.RESULT_CANCELED, null, null) }
        }
        val bar = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(hint)
            addView(cancel)
        }
        val root = FrameLayout(this).apply {
            setBackgroundColor(Color.BLACK)
            addView(preview, FrameLayout.LayoutParams(-1, -1))
            addView(bar, FrameLayout.LayoutParams(-1, -2, Gravity.BOTTOM))
        }
        setContentView(root)
        if (checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
            startCamera()
        } else {
            requestPermissions(arrayOf(Manifest.permission.CAMERA), PERMISSION_REQUEST)
        }
    }

    @Deprecated("ComponentActivity still delivers this for requestPermissions")
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != PERMISSION_REQUEST) return
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) {
            startCamera()
        } else {
            finishWith(Activity.RESULT_CANCELED, null, "camera permission denied")
        }
    }

    private fun startCamera() {
        val future = ProcessCameraProvider.getInstance(this)
        future.addListener({
            val provider = try {
                future.get()
            } catch (e: Exception) {
                finishWith(Activity.RESULT_CANCELED, null, "no camera")
                return@addListener
            }
            val previewCase = Preview.Builder().build().also { it.surfaceProvider = preview.surfaceProvider }
            val analysisCase = ImageAnalysis.Builder()
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .build()
            analysisCase.setAnalyzer(analysis) { image -> analyze(image) }
            try {
                provider.unbindAll()
                provider.bindToLifecycle(this, CameraSelector.DEFAULT_BACK_CAMERA, previewCase, analysisCase)
            } catch (e: Exception) {
                finishWith(Activity.RESULT_CANCELED, null, "no camera")
            }
        }, ContextCompat.getMainExecutor(this))
    }

    private fun analyze(image: ImageProxy) {
        try {
            if (done) return
            val plane = image.planes[0]
            val stride = plane.rowStride
            val buffer = plane.buffer
            // The last row may be shorter than the stride; pad to a full frame.
            val luma = ByteArray(stride * image.height)
            buffer.get(luma, 0, minOf(buffer.remaining(), luma.size))
            val text = decoder.decode(luma, stride, image.width, image.height) ?: return
            done = true
            runOnUiThread { finishWith(Activity.RESULT_OK, text, null) }
        } finally {
            image.close()
        }
    }

    private fun finishWith(code: Int, text: String?, error: String?) {
        val data = Intent()
        text?.let { data.putExtra(EXTRA_TEXT, it) }
        error?.let { data.putExtra(EXTRA_ERROR, it) }
        setResult(code, data)
        finish()
    }

    override fun onDestroy() {
        super.onDestroy()
        analysis.shutdown()
    }
}
