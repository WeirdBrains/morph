package org.weirdbrains.morph

import android.app.Activity
import android.app.role.RoleManager
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel

/**
 * Default-browser status and request. Android 10+ asks through
 * RoleManager.ROLE_BROWSER (a system dialog); older versions, or a role the
 * system will not grant, open the default-apps screen instead.
 */
object DefaultBrowser {
    const val CHANNEL = "org.weirdbrains.morph/default_browser"
    const val REQUEST_CODE = 0x6d62 // "mb"

    private var pending: MethodChannel.Result? = null

    fun register(activity: Activity, messenger: BinaryMessenger) {
        MethodChannel(messenger, CHANNEL).setMethodCallHandler { call, result ->
            when (call.method) {
                "status" -> result.success(isDefault(activity))
                "request" -> request(activity, result)
                else -> result.notImplemented()
            }
        }
    }

    fun isDefault(activity: Activity): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val roles = activity.getSystemService(RoleManager::class.java)
            if (roles != null && roles.isRoleAvailable(RoleManager.ROLE_BROWSER)) {
                return roles.isRoleHeld(RoleManager.ROLE_BROWSER)
            }
        }
        val view = Intent(Intent.ACTION_VIEW, Uri.parse("https://example.com"))
            .addCategory(Intent.CATEGORY_BROWSABLE)
        val info = activity.packageManager.resolveActivity(view, PackageManager.MATCH_DEFAULT_ONLY)
        return info?.activityInfo?.packageName == activity.packageName
    }

    private fun request(activity: Activity, result: MethodChannel.Result) {
        if (isDefault(activity)) {
            result.success("madeDefault")
            return
        }
        if (pending != null) {
            result.error("busy", "a request is already open", null)
            return
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val roles = activity.getSystemService(RoleManager::class.java)
            if (roles != null && roles.isRoleAvailable(RoleManager.ROLE_BROWSER)) {
                pending = result
                try {
                    activity.startActivityForResult(
                        roles.createRequestRoleIntent(RoleManager.ROLE_BROWSER),
                        REQUEST_CODE,
                    )
                    return
                } catch (e: ActivityNotFoundException) {
                    pending = null
                }
            }
        }
        result.success(if (openSettings(activity)) "openedSettings" else "failed")
    }

    private fun openSettings(activity: Activity): Boolean {
        for (action in listOf(Settings.ACTION_MANAGE_DEFAULT_APPS_SETTINGS, Settings.ACTION_SETTINGS)) {
            try {
                activity.startActivity(Intent(action))
                return true
            } catch (e: ActivityNotFoundException) {
                // Try the next screen.
            }
        }
        return false
    }

    /** Returns true when the result was ours. */
    fun onActivityResult(activity: Activity, requestCode: Int): Boolean {
        if (requestCode != REQUEST_CODE) {
            return false
        }
        val result = pending ?: return true
        pending = null
        if (isDefault(activity)) {
            result.success("madeDefault")
        } else {
            // The system dialog stops offering itself after repeated "no";
            // the settings screen always works.
            result.success("declined")
        }
        return true
    }
}
