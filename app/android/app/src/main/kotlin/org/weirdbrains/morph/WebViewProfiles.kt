package org.weirdbrains.morph

import android.util.Log
import android.view.View
import android.webkit.WebView
import androidx.webkit.ProfileStore
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.util.concurrent.ConcurrentHashMap

/**
 * Gives each Morph profile its own Android WebView profile (androidx.webkit
 * MULTI_PROFILE): separate cookies, local/session storage, IndexedDB, cache
 * and service workers.
 *
 * Dart registers `keepAliveId -> profileId` before it builds the
 * flutter_inappwebview widget. The plugin names its InAppWebView after that
 * keep-alive id and calls Activity.registerForContextMenu from the WebView
 * constructor, before it touches settings, scripts or the network. MainActivity
 * forwards that call here, so the profile is set while the WebView is fresh.
 */
object WebViewProfiles {
    private const val TAG = "MorphProfiles"
    const val CHANNEL = "org.weirdbrains.morph_webview/profile"

    private val pending = ConcurrentHashMap<String, String>()

    /** Morph's default profile keeps using the WebView default store. */
    fun androidProfileName(profileId: String): String? =
        if (profileId.isEmpty() || profileId == "default") null else "morph-$profileId"

    fun isSupported(): Boolean = WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)

    fun register(messenger: BinaryMessenger) {
        MethodChannel(messenger, CHANNEL).setMethodCallHandler(::onMethodCall)
    }

    private fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "prepareAndroid" -> {
                val profileId = call.argument<String>("profileId") ?: "default"
                val viewKey = call.argument<String>("viewKey")
                val name = androidProfileName(profileId)
                if (name == null) {
                    result.success(mapOf("isolated" to true, "profile" to "Default"))
                    return
                }
                if (!isSupported()) {
                    result.success(mapOf("isolated" to false, "profile" to "Default"))
                    return
                }
                ProfileStore.getInstance().getOrCreateProfile(name)
                if (viewKey != null) {
                    pending[viewKey] = name
                }
                result.success(mapOf("isolated" to true, "profile" to name))
            }
            "forgetView" -> {
                call.argument<String>("viewKey")?.let { pending.remove(it) }
                result.success(null)
            }
            "deleteAndroid" -> {
                val name = androidProfileName(call.argument<String>("profileId") ?: "")
                if (name == null || !isSupported()) {
                    result.success(false)
                    return
                }
                try {
                    result.success(ProfileStore.getInstance().deleteProfile(name))
                } catch (e: IllegalStateException) {
                    // Still in use by a live WebView; Morph retries on next start.
                    Log.w(TAG, "profile $name still in use: ${e.message}")
                    result.success(false)
                }
            }
            "listAndroid" -> {
                if (!isSupported()) {
                    result.success(emptyList<String>())
                } else {
                    result.success(ProfileStore.getInstance().allProfileNames)
                }
            }
            else -> result.notImplemented()
        }
    }

    /** Called for every view registered for a context menu. */
    fun onViewConstructed(view: View) {
        if (view !is WebView) {
            return
        }
        val key = viewKeyOf(view) ?: return
        val name = pending.remove(key) ?: return
        try {
            WebViewCompat.setProfile(view, name)
        } catch (e: IllegalStateException) {
            Log.e(TAG, "could not set profile $name", e)
        }
    }

    /** flutter_inappwebview's InAppWebView keeps its keep-alive id in `id`. */
    private fun viewKeyOf(view: WebView): String? =
        try {
            view.javaClass.getField("id").get(view) as? String
        } catch (_: ReflectiveOperationException) {
            null
        }
}
