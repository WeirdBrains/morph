import 'dart:async';

import 'events.dart';
import 'platform.dart';

/// Public handle used by the Morph shell and by widget tests (via a fake session).
class MorphWebViewController {
  MorphWebViewController({this.initialUrl = 'https://example.com'});

  final String initialUrl;

  MorphWebViewSession? _session;
  MorphNavigationEvent _navigation = const MorphNavigationEvent(url: '');
  MorphRenderPathInfo _renderPath = const MorphRenderPathInfo(
    path: 'uninitialized',
    reason: 'not created',
  );
  final _navController = StreamController<MorphNavigationEvent>.broadcast();
  final _pageController = StreamController<String>.broadcast();
  final _subs = <StreamSubscription<dynamic>>[];

  MorphNavigationEvent get currentNavigation => _navigation;
  MorphRenderPathInfo get renderPath => _renderPath;
  int? get textureId => _session?.textureId;
  bool get isAttached => _session != null;

  Stream<MorphNavigationEvent> get navigation => _navController.stream;
  Stream<String> get pageMessages => _pageController.stream;

  Future<void> attach({
    required int width,
    required int height,
    required double devicePixelRatio,
    String userScript = '',
  }) async {
    if (_session != null) {
      await resize(
        width: width,
        height: height,
        devicePixelRatio: devicePixelRatio,
      );
      return;
    }
    final session = await MorphWebViewPlatform.instance.create(
      initialUrl: initialUrl,
      width: width,
      height: height,
      devicePixelRatio: devicePixelRatio,
      userScript: userScript,
    );
    _session = session;
    _renderPath = session.renderPath;
    _subs.add(
      session.navigation.listen((event) {
        _navigation = event;
        _navController.add(event);
      }),
    );
    _subs.add(session.pageMessages.listen(_pageController.add));
  }

  Future<void> resize({
    required int width,
    required int height,
    required double devicePixelRatio,
  }) async {
    await _session?.resize(
      width: width,
      height: height,
      devicePixelRatio: devicePixelRatio,
    );
  }

  Future<void> loadUrl(String url) async => _session?.loadUrl(url);
  Future<void> goBack() async => _session?.goBack();
  Future<void> goForward() async => _session?.goForward();
  Future<void> reload() async => _session?.reload();
  Future<void> sendPointer(MorphPointerEvent event) async =>
      _session?.sendPointer(event);
  Future<void> sendKey(MorphKeyEvent event) async => _session?.sendKey(event);
  Future<String?> evaluateJavaScript(String script) async =>
      _session?.evaluateJavaScript(script);

  Future<void> dispose() async {
    for (final sub in _subs) {
      await sub.cancel();
    }
    _subs.clear();
    await _session?.dispose();
    _session = null;
    await _navController.close();
    await _pageController.close();
  }
}
