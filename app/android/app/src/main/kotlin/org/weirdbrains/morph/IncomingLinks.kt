package org.weirdbrains.morph

import android.content.Intent
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel

/**
 * What other apps hand Morph: http(s) links (ACTION_VIEW) and shared or
 * selected text (ACTION_SEND, ACTION_SEND_MULTIPLE, ACTION_PROCESS_TEXT).
 * Dart decides whether shared text is a link or a search. The launch
 * intent waits until Dart asks for it; later ones are pushed as they arrive.
 */
object IncomingLinks {
    const val CHANNEL = "org.weirdbrains.morph/links"

    private var channel: MethodChannel? = null
    private var initialLink: String? = null
    private var initialShare: String? = null

    fun register(messenger: BinaryMessenger, launch: Intent?) {
        initialLink = linkOf(launch)
        initialShare = sharedTextOf(launch)
        channel = MethodChannel(messenger, CHANNEL).also {
            it.setMethodCallHandler { call, result ->
                when (call.method) {
                    "initialLink" -> {
                        result.success(initialLink)
                        initialLink = null
                    }
                    "initialShare" -> {
                        result.success(initialShare)
                        initialShare = null
                    }
                    else -> result.notImplemented()
                }
            }
        }
    }

    fun onNewIntent(intent: Intent) {
        linkOf(intent)?.let { channel?.invokeMethod("open", it) }
        sharedTextOf(intent)?.let { channel?.invokeMethod("share", it) }
    }

    fun linkOf(intent: Intent?): String? {
        if (intent?.action != Intent.ACTION_VIEW) {
            return null
        }
        val data = intent.data ?: return null
        val scheme = data.scheme?.lowercase()
        return if (scheme == "http" || scheme == "https") data.toString() else null
    }

    /** The text another app shared or selected, or null. */
    fun sharedTextOf(intent: Intent?): String? {
        val text = when (intent?.action) {
            Intent.ACTION_SEND ->
                intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
                    ?: intent.getCharSequenceExtra(Intent.EXTRA_SUBJECT)?.toString()
            Intent.ACTION_SEND_MULTIPLE ->
                intent.getCharSequenceArrayListExtra(Intent.EXTRA_TEXT)
                    ?.joinToString("\n")
                    ?: intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
            Intent.ACTION_PROCESS_TEXT ->
                intent.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString()
            else -> null
        }
        return text?.trim()?.takeIf { it.isNotEmpty() }
    }
}
