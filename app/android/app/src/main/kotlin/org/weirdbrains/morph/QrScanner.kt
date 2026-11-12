package org.weirdbrains.morph

import android.app.Activity
import android.content.Intent
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel

/**
 * `org.weirdbrains.morph/scan`: `scan` opens [QrScanActivity] and answers
 * with the scanned text, or null when the user backed out.
 */
object QrScanner {
    const val CHANNEL = "org.weirdbrains.morph/scan"
    const val REQUEST_CODE = 0x7173 // "qs"

    private var pending: MethodChannel.Result? = null

    fun register(activity: Activity, messenger: BinaryMessenger) {
        MethodChannel(messenger, CHANNEL).setMethodCallHandler { call, result ->
            if (call.method != "scan") {
                result.notImplemented()
                return@setMethodCallHandler
            }
            if (pending != null) {
                result.error("busy", "the scanner is already open", null)
                return@setMethodCallHandler
            }
            pending = result
            @Suppress("DEPRECATION")
            activity.startActivityForResult(Intent(activity, QrScanActivity::class.java), REQUEST_CODE)
        }
    }

    /** True when the result was ours. */
    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST_CODE) return false
        val result = pending ?: return true
        pending = null
        result.success(if (resultCode == Activity.RESULT_OK) data?.getStringExtra(QrScanActivity.EXTRA_TEXT) else null)
        return true
    }
}
