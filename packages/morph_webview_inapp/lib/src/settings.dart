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
///
/// Scrolling (Android): a visible tab pre-rasters the page beyond the
/// viewport (`setOffscreenPreRaster`), so a fast fling over a long page lands
/// on painted tiles instead of blank ones that fill in a frame or more late,
/// and asks for an IMPORTANT renderer that is never waived. With texture-layer
/// composition the WebView always counts as attached and visible to Android,
/// so the system cannot lower a hidden tab on its own: [idle] (a tab hidden
/// for a while) drops the pre-raster and waives its renderer priority so its
/// memory can go. The shell pairs idle with `WebView.onPause`.
InAppWebViewSettings morphInAppWebViewSettings({
  bool? desktopSite,
  bool idle = false,
}) {
  return InAppWebViewSettings(
    offscreenPreRaster: !idle,
    rendererPriorityPolicy: RendererPriorityPolicy(
      rendererRequestedPriority: idle
          ? RendererPriority.RENDERER_PRIORITY_WAIVED
          : RendererPriority.RENDERER_PRIORITY_IMPORTANT,
      waivedWhenNotVisible: idle,
    ),
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
    // Android: report a dead renderer so the tab shows its crash state and
    // can be rebuilt, instead of the whole app being killed.
    useOnRenderProcessGone: true,
    // Android: failed loads show Morph's own error page (MorphLoadErrorGate),
    // not Chromium's, so every platform shows the same pages.
    disableDefaultErrorPage: true,
    // window.open and target=_blank open a real second web view (see
    // popups.dart). Without this, Android loads the new window's address
    // into the tab itself and the page loses window.opener: Google sign-in
    // then shows accounts.google.com/gsi/select as a blank white page.
    supportMultipleWindows: true,
    javaScriptCanOpenWindowsAutomatically: true,
    preferredContentMode: switch (desktopSite) {
      null => UserPreferredContentMode.RECOMMENDED,
      true => UserPreferredContentMode.DESKTOP,
      false => UserPreferredContentMode.MOBILE,
    },
    useWideViewPort: desktopSite ?? true,
    loadWithOverviewMode: desktopSite ?? true,
  );
}
