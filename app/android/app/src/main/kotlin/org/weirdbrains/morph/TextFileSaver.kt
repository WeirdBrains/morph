package org.weirdbrains.morph

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel

/**
 * Saves a small text file where the user picks (Storage Access Framework,
 * ACTION_CREATE_DOCUMENT), so Morph needs no storage permission. The text is
 * held only until the picker returns.
 */
object TextFileSaver {
    const val CHANNEL = "org.weirdbrains.morph/save"
    const val REQUEST_CODE = 0x6d73 // "ms"

    private var pendingText: String? = null
    private var pendingResult: MethodChannel.Result? = null

    fun register(activity: Activity, messenger: BinaryMessenger) {
        MethodChannel(messenger, CHANNEL).setMethodCallHandler { call, result ->
            if (call.method != "saveText") {
                result.notImplemented()
                return@setMethodCallHandler
            }
            if (pendingResult != null) {
                result.error("busy", "a save is already open", null)
                return@setMethodCallHandler
            }
            val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = "text/plain"
                putExtra(Intent.EXTRA_TITLE, call.argument<String>("fileName") ?: "morph.txt")
            }
            pendingText = call.argument<String>("text").orEmpty()
            pendingResult = result
            try {
                activity.startActivityForResult(intent, REQUEST_CODE)
            } catch (e: ActivityNotFoundException) {
                finish(false)
            }
        }
    }

    /** Returns true when the result was ours. */
    fun onActivityResult(activity: Activity, requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST_CODE) {
            return false
        }
        val uri = data?.data
        if (resultCode != Activity.RESULT_OK || uri == null) {
            finish(false)
            return true
        }
        val ok = try {
            activity.contentResolver.openOutputStream(uri, "wt")?.use {
                it.write(pendingText.orEmpty().toByteArray(Charsets.UTF_8))
                true
            } ?: false
        } catch (e: Exception) {
            false
        }
        finish(ok)
        return true
    }

    private fun finish(saved: Boolean) {
        pendingText = null
        val result = pendingResult
        pendingResult = null
        result?.success(saved)
    }
}
