import 'dart:async';

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
    // Emit the first navigation on the next microtask so listeners can attach.
    scheduleMicrotask(() => _navController.add(_nav));
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
  final List<String> history = [];
  int historyIndex = -1;
  final List<MorphPointerEvent> pointers = [];
  final List<MorphKeyEvent> keys = [];
  final List<String> evaluated = [];

  @override
  Stream<MorphNavigationEvent> get navigation => _navController.stream;

  @override
  Stream<String> get pageMessages => _pageController.stream;

  void emitPageMessage(String json) => _pageController.add(json);

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
  Future<String?> evaluateJavaScript(String script) async {
    evaluated.add(script);
    if (script.contains('MorphBridge.ping')) {
      emitPageMessage('{"id":"1","type":"ping"}');
      return '';
    }
    return '';
  }

  @override
  Future<void> dispose() async {
    await _navController.close();
    await _pageController.close();
  }
}
