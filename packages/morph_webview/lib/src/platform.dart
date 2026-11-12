import 'dart:typed_data';

import 'package:flutter/widgets.dart';
import 'package:plugin_platform_interface/plugin_platform_interface.dart';

import 'events.dart';

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
  });
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
  }) {
    throw UnimplementedError(
      'MorphWebViewPlatform.create has not been set. '
      'Widget tests should install a fake; desktop apps use a morph_webview platform package.',
    );
  }
}
