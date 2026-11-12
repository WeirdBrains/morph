package org.weirdbrains.morph

import android.content.Intent
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel

/**
 * http(s) links other apps open in Morph (ACTION_VIEW). The launch link
 * waits until Dart asks for it; later ones are pushed as they arrive.
 */
object IncomingLinks {
    const val CHANNEL = "org.weirdbrains.morph/links"

    private var channel: MethodChannel? = null
    private var initial: String? = null

    fun register(messenger: BinaryMessenger, launch: Intent?) {
        initial = linkOf(launch)
        channel = MethodChannel(messenger, CHANNEL).also {
            it.setMethodCallHandler { call, result ->
                if (call.method == "initialLink") {
                    result.success(initial)
                    initial = null
                } else {
                    result.notImplemented()
                }
            }
        }
    }

    fun onNewIntent(intent: Intent) {
        val url = linkOf(intent) ?: return
        channel?.invokeMethod("open", url)
    }

    fun linkOf(intent: Intent?): String? {
        if (intent?.action != Intent.ACTION_VIEW) {
            return null
        }
        val data = intent.data ?: return null
        val scheme = data.scheme?.lowercase()
        return if (scheme == "http" || scheme == "https") data.toString() else null
    }
}
