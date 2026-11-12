import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/widgets.dart';

import 'events.dart';
import 'platform.dart';

/// In-memory backend for widget tests. Never loads Native Assets or WebKit.
class FakeMorphWebViewPlatform extends MorphWebViewPlatform {
  FakeMorphWebViewPlatform({
    this.renderPath = const MorphRenderPathInfo(
      path: 'webkitgtk-snapshot',
      reason: 'fake',
    ),
    this.platformView,
  });

  final MorphRenderPathInfo renderPath;
  final Widget? platformView;
  final List<String> loadedUrls = [];
  final List<String> evaluated = [];
  FakeMorphWebViewSession? lastSession;

  @override
  Future<MorphWebViewSession> create({
    required String initialUrl,
    required int width,
    required int height,
    required double devicePixelRatio,
    String userScript = '',
  }) async {
    loadedUrls.add(initialUrl);
    final session = FakeMorphWebViewSession(
      renderPath: renderPath,
      url: initialUrl,
      textureId: platformView == null ? 1 : null,
      platformView: platformView,
    );
    lastSession = session;
    return session;
  }
}

class FakeMorphWebViewSession extends MorphWebViewSession {
  FakeMorphWebViewSession({
    required this.renderPath,
    required String url,
    this.textureId = 1,
    this.platformView,
  }) : _nav = MorphNavigationEvent(url: url, title: 'Fake', progress: 1) {
    history.add(url);
    historyIndex = 0;
  }

  @override
  final int? textureId;

  @override
  final Widget? platformView;

  @override
  final MorphRenderPathInfo renderPath;

  MorphNavigationEvent _nav;
  final _navController = StreamController<MorphNavigationEvent>.broadcast();
  final _pageController = StreamController<String>.broadcast();
  final _cursorController = StreamController<MorphCursorEvent>.broadcast();
  final List<String> history = [];
  int historyIndex = -1;
  final List<MorphPointerEvent> pointers = [];
  final List<MorphKeyEvent> keys = [];
  final List<MorphImeEvent> imes = [];
  final List<String> evaluated = [];

  @override
  Stream<MorphNavigationEvent> get navigation {
    // Schedule the initial event when a listener attaches, not in the
    // constructor: async create() would otherwise publish it before
    // MorphWebViewController can subscribe.
    if (!_initialNavScheduled) {
      _initialNavScheduled = true;
      scheduleMicrotask(() {
        if (!_navController.isClosed) {
          _navController.add(_nav);
        }
      });
    }
    return _navController.stream;
  }

  bool _initialNavScheduled = false;

  @override
  Stream<String> get pageMessages => _pageController.stream;

  @override
  Stream<MorphCursorEvent> get cursor => _cursorController.stream;

  void emitPageMessage(String json) => _pageController.add(json);

  void emitCursor(String shape) =>
      _cursorController.add(MorphCursorEvent(shape: shape));

  void _push(String url, {String title = 'Fake'}) {
    if (historyIndex < history.length - 1) {
      history.removeRange(historyIndex + 1, history.length);
    }
    history.add(url);
    historyIndex = history.length - 1;
    _nav = MorphNavigationEvent(
      url: url,
      title: title,
      canGoBack: historyIndex > 0,
      canGoForward: historyIndex < history.length - 1,
      progress: 1,
    );
    _navController.add(_nav);
  }

  @override
  Future<void> resize({
    required int width,
    required int height,
    required double devicePixelRatio,
  }) async {}

  @override
  Future<void> loadUrl(String url) async {
    if (historyIndex < 0) {
      _push(url);
    } else {
      _push(url);
    }
  }

  @override
  Future<void> goBack() async {
    if (historyIndex > 0) {
      historyIndex--;
      _nav = MorphNavigationEvent(
        url: history[historyIndex],
        title: 'Fake',
        canGoBack: historyIndex > 0,
        canGoForward: true,
        progress: 1,
      );
      _navController.add(_nav);
    }
  }

  @override
  Future<void> goForward() async {
    if (historyIndex < history.length - 1) {
      historyIndex++;
      _nav = MorphNavigationEvent(
        url: history[historyIndex],
        title: 'Fake',
        canGoBack: true,
        canGoForward: historyIndex < history.length - 1,
        progress: 1,
      );
      _navController.add(_nav);
    }
  }

  @override
  Future<void> reload() async {
    _navController.add(_nav);
  }

  @override
  Future<void> sendPointer(MorphPointerEvent event) async =>
      pointers.add(event);

  @override
  Future<void> sendKey(MorphKeyEvent event) async => keys.add(event);

  @override
  Future<void> sendIme(MorphImeEvent event) async => imes.add(event);

  @override
  Future<String?> evaluateJavaScript(String script) async {
    evaluated.add(script);
    if (script.contains('MorphBridge.ping')) {
      emitPageMessage('{"id":"1","type":"ping"}');
      return '';
    }
    return '';
  }

  bool _desktopSite = false;
  Uint8List? screenshotBytes;

  @override
  bool get desktopSite => _desktopSite;

  @override
  Future<void> setDesktopSite(bool enabled) async {
    _desktopSite = enabled;
  }

  @override
  Future<Uint8List?> takeScreenshot() async {
    // A 1x1 PNG so tab-switcher tests have a preview to render.
    return screenshotBytes ??
        Uint8List.fromList(const [
          0x89,
          0x50,
          0x4E,
          0x47,
          0x0D,
          0x0A,
          0x1A,
          0x0A,
          0x00,
          0x00,
          0x00,
          0x0D,
          0x49,
          0x48,
          0x44,
          0x52,
          0x00,
          0x00,
          0x00,
          0x01,
          0x00,
          0x00,
          0x00,
          0x01,
          0x08,
          0x02,
          0x00,
          0x00,
          0x00,
          0x90,
          0x77,
          0x53,
          0xDE,
          0x00,
          0x00,
          0x00,
          0x0C,
          0x49,
          0x44,
          0x41,
          0x54,
          0x08,
          0xD7,
          0x63,
          0xF8,
          0xCF,
          0xC0,
          0x00,
          0x00,
          0x00,
          0x03,
          0x00,
          0x01,
          0x00,
          0x05,
          0xFE,
          0xD4,
          0xEF,
          0x00,
          0x00,
          0x00,
          0x00,
          0x49,
          0x45,
          0x4E,
          0x44,
          0xAE,
          0x42,
          0x60,
          0x82,
        ]);
  }

  @override
  Future<void> dispose() async {
    await _navController.close();
    await _pageController.close();
    await _cursorController.close();
  }
}
