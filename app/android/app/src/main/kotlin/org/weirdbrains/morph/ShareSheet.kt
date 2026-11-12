package org.weirdbrains.morph

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel

/** Opens the Android share sheet for a page link (ACTION_SEND, text/plain). */
object ShareSheet {
    const val CHANNEL = "org.weirdbrains.morph/share"

    fun register(activity: Activity, messenger: BinaryMessenger) {
        MethodChannel(messenger, CHANNEL).setMethodCallHandler { call, result ->
            if (call.method != "shareText") {
                result.notImplemented()
                return@setMethodCallHandler
            }
            val text = call.argument<String>("text").orEmpty()
            if (text.isBlank()) {
                result.success(false)
                return@setMethodCallHandler
            }
            val send = Intent(Intent.ACTION_SEND).apply {
                type = "text/plain"
                putExtra(Intent.EXTRA_TEXT, text)
                call.argument<String>("subject")?.takeIf { it.isNotBlank() }?.let {
                    putExtra(Intent.EXTRA_SUBJECT, it)
                }
            }
            try {
                activity.startActivity(Intent.createChooser(send, null))
                result.success(true)
            } catch (e: ActivityNotFoundException) {
                result.success(false)
            }
        }
    }
}
