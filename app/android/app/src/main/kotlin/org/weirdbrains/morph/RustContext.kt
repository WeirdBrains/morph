package org.weirdbrains.morph

import android.content.Context

/**
 * Hands the application Context to the Rust bridge (ndk-context), which the
 * networking stack needs to read Android's DNS servers. Without it, starting
 * a Weirdnet node (QR device linking) fails. Safe to call more than once.
 */
object RustContext {
    private var initialized = false

    fun init(context: Context) {
        if (initialized) return
        System.loadLibrary("morph_bridge")
        nativeInit(context.applicationContext)
        initialized = true
    }

    @JvmStatic
    private external fun nativeInit(context: Context)
}
