import 'package:flutter_inappwebview/flutter_inappwebview.dart';

/// The one place Morph builds [InAppWebViewSettings].
///
/// Every call to `setSettings` must start from this, because a fresh
/// [InAppWebViewSettings] carries the plugin defaults and would silently undo
/// the Android composition choices below.
///
/// Android composition: the plugin default is hybrid composition with the
/// WebView on a `LAYER_TYPE_HARDWARE` layer. On Flutter 3.47 that hardware
/// layer stops being repainted after the view is resized (keyboard shown and
/// hidden by the address sheet), after a second page load, or after the app
/// comes back from the background, so the page is loaded and live but the
/// screen shows a flat white rectangle that ignores swipes. See
/// pichillilorenzo/flutter_inappwebview#2889 and #2950. Texture-layer hybrid
/// composition with no forced hardware layer (the plain Android WebView
/// default) keeps painting in all of those cases.
///
/// [desktopSite] is null at creation so each platform keeps its recommended
/// content mode; the desktop-site toggle passes true or false.
InAppWebViewSettings morphInAppWebViewSettings({bool? desktopSite}) {
  return InAppWebViewSettings(
    javaScriptEnabled: true,
    isInspectable: true,
    mediaPlaybackRequiresUserGesture: false,
    allowsBackForwardNavigationGestures: true,
    // Persist cookies in the profile's store, not an incognito session.
    // Windows tabs in the same profile share a WebView2 userDataFolder;
    // macOS 14+ uses a per-profile WKWebsiteDataStore when the hook is
    // installed. Android still uses the process-wide default store.
    incognito: false,
    cacheEnabled: true,
    sharedCookiesEnabled: true,
    thirdPartyCookiesEnabled: true,
    // Android: texture-layer hybrid composition, no forced hardware layer.
    useHybridComposition: false,
    hardwareAcceleration: false,
    // Android: let Morph rebuild the view when the renderer dies instead of
    // leaving a dead white surface.
    useOnRenderProcessGone: true,
    preferredContentMode: switch (desktopSite) {
      null => UserPreferredContentMode.RECOMMENDED,
      true => UserPreferredContentMode.DESKTOP,
      false => UserPreferredContentMode.MOBILE,
    },
    useWideViewPort: desktopSite ?? true,
    loadWithOverviewMode: desktopSite ?? true,
  );
}
