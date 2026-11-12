import 'dart:typed_data';

import 'package:flutter/widgets.dart';
import 'package:plugin_platform_interface/plugin_platform_interface.dart';

import 'events.dart';
import 'surface_health.dart';

/// Backend contract implemented by `morph_webview_linux` (and later other platforms).
abstract class MorphWebViewPlatform extends PlatformInterface {
  MorphWebViewPlatform() : super(token: _token);

  static final Object _token = Object();

  static MorphWebViewPlatform _instance = _UnimplementedWebViewPlatform();

  static MorphWebViewPlatform get instance => _instance;

  static set instance(MorphWebViewPlatform instance) {
    PlatformInterface.verifyToken(instance, _token);
    _instance = instance;
  }

  Future<MorphWebViewSession> create({
    required String initialUrl,
    required int width,
    required int height,
    required double devicePixelRatio,
    String userScript = '',
    String profileId = 'default',
  });

  /// Drop any engine-side storage the backend keeps for [profileId] after
  /// Morph deletes the profile. Backends that store profiles under the Morph
  /// home (removed by morph-core) need nothing here.
  Future<void> deleteProfileData(String profileId) async {}

  /// Drop engine-side storage for any profile not in [liveProfileIds], such
  /// as one deleted while its pages were still open.
  Future<void> pruneProfileData(Iterable<String> liveProfileIds) async {}
}

/// One live view on a platform backend.
abstract class MorphWebViewSession {
  int? get textureId;

  /// When non-null, [MorphWebView] embeds this widget instead of a texture.
  Widget? get platformView => null;
  MorphRenderPathInfo get renderPath;

  Stream<MorphNavigationEvent> get navigation;
  Stream<String> get pageMessages;
  Stream<MorphCursorEvent> get cursor => const Stream.empty();

  /// When true, the control layer already ran the page-bridge hook and delivered
  /// any reply; the shell should not call `handlePageMessage` again.
  bool get handlesPageBridge => false;

  Future<void> resize({
    required int width,
    required int height,
    required double devicePixelRatio,
  });
  Future<void> loadUrl(String url);
  Future<void> goBack();
  Future<void> goForward();
  Future<void> reload();
  Future<void> sendPointer(MorphPointerEvent event);
  Future<void> sendKey(MorphKeyEvent event);
  Future<void> sendIme(MorphImeEvent event) async {}
  Future<String?> evaluateJavaScript(String script);

  /// A PNG of the current page, or null when the backend cannot capture one.
  Future<Uint8List?> takeScreenshot() async => null;

  /// Request a desktop or mobile user-agent / content mode.
  Future<void> setDesktopSite(bool enabled) async {}

  bool get desktopSite => false;

  /// Whether the native surface is painting the page. Backends that cannot
  /// tell return [MorphSurfaceVerdict.unknown].
  Future<MorphSurfaceVerdict> checkSurface() async =>
      MorphSurfaceVerdict.unknown;

  /// Cheap nudge that makes the native view repaint without touching the
  /// page. Safe to call often.
  Future<void> repaintSurface() async {}

  /// Replaces the native view with a fresh one on the current URL. Returns
  /// false when the backend cannot rebuild its view.
  Future<bool> rebuildSurface() async => false;

  /// Fires when the backend learns its surface is gone for good (for example
  /// the Android renderer process died) and must be rebuilt.
  Stream<void> get surfaceLost => const Stream.empty();

  /// Renderer crashes, kills and hangs for this view only. Other tabs keep
  /// running; the shell shows a per-tab "page crashed" state.
  Stream<MorphRendererEvent> get renderer => const Stream.empty();

  /// Ends a hung renderer so the tab can be reloaded.
  Future<void> terminateRenderer() async {}

  /// Brings the view back after its renderer died: a fresh native view where
  /// the old one is unusable, else a reload. The controller then loads the
  /// last URL.
  Future<void> recoverRenderer() async {
    if (!await rebuildSurface()) {
      await reload();
    }
  }

  Future<void> dispose();
}

class _UnimplementedWebViewPlatform extends MorphWebViewPlatform {
  @override
  Future<MorphWebViewSession> create({
    required String initialUrl,
    required int width,
    required int height,
    required double devicePixelRatio,
    String userScript = '',
    String profileId = 'default',
  }) {
    throw UnimplementedError(
      'MorphWebViewPlatform.create has not been set. '
      'Widget tests should install a fake; desktop apps use a morph_webview platform package.',
    );
  }
}
