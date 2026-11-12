package org.weirdbrains.morph

import android.view.View
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        WebViewProfiles.register(flutterEngine.dartExecutor.binaryMessenger)
        ShareSheet.register(this, flutterEngine.dartExecutor.binaryMessenger)
    }

    // flutter_inappwebview registers each new WebView here from its
    // constructor, before it is configured or navigated.
    override fun registerForContextMenu(view: View) {
        WebViewProfiles.onViewConstructed(view)
        super.registerForContextMenu(view)
    }
}
