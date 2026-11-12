import 'package:flutter_inappwebview/flutter_inappwebview.dart';

/// The one place Morph builds [InAppWebViewSettings].
///
/// Every call to `setSettings` must start from this, because a fresh
/// [InAppWebViewSettings] carries the plugin defaults and would silently undo
/// the Android composition choices below.
///
/// Android composition: hybrid composition (the native WebView sits in the
/// Android view hierarchy and SurfaceFlinger composites it) with no forced
/// hardware layer. Texture-layer composition, used before, has the WebView
/// draw into a SurfaceTexture that Flutter's raster thread samples on its own
/// clock, so during a fling the page repeats a frame and then jumps two to
/// five frames ahead whenever the two clocks drift (Pixel 9 Pro XL: 44% of
/// fast-fling frames out of step, single-frame jumps of 4-5 frames' worth).
/// The plugin default pairs hybrid composition with `LAYER_TYPE_HARDWARE`,
/// and on Flutter 3.47 that hardware layer stops being repainted after the
/// view is resized (keyboard shown and hidden by the address sheet), after a
/// second page load, or after the app comes back from the background: a flat
/// white rectangle that ignores swipes (pichillilorenzo/flutter_inappwebview
/// #2889, #2950). Without the forced layer (`LAYER_TYPE_NONE`, the plain
/// Android WebView default) it keeps painting in all of those cases.
///
/// [desktopSite] is null at creation so each platform keeps its recommended
/// content mode; the desktop-site toggle passes true or false.
///
/// Scrolling (Android): a visible tab pre-rasters the page beyond the
/// viewport (`setOffscreenPreRaster`), so a fast fling over a long page lands
/// on painted tiles instead of blank ones that fill in a frame or more late,
/// and asks for an IMPORTANT renderer that is never waived. Hidden tabs stay
/// mounted (Offstage), so Morph lowers them itself: [idle] (a tab hidden for
/// a while) drops the pre-raster and waives its renderer priority so its
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
    // Android: hybrid composition, no forced hardware layer (see above).
    useHybridComposition: true,
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
