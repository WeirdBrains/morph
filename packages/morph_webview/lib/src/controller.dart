import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/widgets.dart';

import 'events.dart';
import 'platform.dart';

/// Public handle used by the Morph shell and by widget tests (via a fake session).
class MorphWebViewController {
  MorphWebViewController({this.initialUrl = 'https://example.com'});

  final String initialUrl;

  MorphWebViewSession? _session;
  String? _pendingUrl;
  MorphNavigationEvent _navigation = const MorphNavigationEvent(url: '');
  MorphRenderPathInfo _renderPath = const MorphRenderPathInfo(
    path: 'uninitialized',
    reason: 'not created',
  );
  final focusNode = FocusNode(debugLabel: 'morph-webview');
  final cursorShape = ValueNotifier<String>('default');
  final _navController = StreamController<MorphNavigationEvent>.broadcast();
  final _pageController = StreamController<String>.broadcast();
  final _cursorController = StreamController<MorphCursorEvent>.broadcast();
  final _subs = <StreamSubscription<dynamic>>[];

  MorphNavigationEvent get currentNavigation => _navigation;
  MorphRenderPathInfo get renderPath => _renderPath;
  int? get textureId => _session?.textureId;
  Widget? get platformView => _session?.platformView;
  bool get isAttached => _session != null;
  bool get handlesPageBridge => _session?.handlesPageBridge ?? false;

  Stream<MorphNavigationEvent> get navigation => _navController.stream;
  Stream<String> get pageMessages => _pageController.stream;
  Stream<MorphCursorEvent> get cursor => _cursorController.stream;

  void requestFocus() => focusNode.requestFocus();

  Future<void> attach({
    required int width,
    required int height,
    required double devicePixelRatio,
    String userScript = '',
    String profileId = 'default',
  }) async {
    if (_session != null) {
      await resize(
        width: width,
        height: height,
        devicePixelRatio: devicePixelRatio,
      );
      return;
    }
    final url = _pendingUrl ?? initialUrl;
    _pendingUrl = null;
    final session = await MorphWebViewPlatform.instance.create(
      initialUrl: url,
      width: width,
      height: height,
      devicePixelRatio: devicePixelRatio,
      userScript: userScript,
      profileId: profileId,
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
    _subs.add(
      session.cursor.listen((event) {
        cursorShape.value = event.shape;
        _cursorController.add(event);
      }),
    );
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

  Future<void> loadUrl(String url) async {
    if (_session == null) {
      // The shell can submit an address before the first platform view
      // attaches (common on Android while the system WebView starts).
      _pendingUrl = url;
      return;
    }
    await _session!.loadUrl(url);
    // After a user-initiated navigation (address-bar submit), focus the page
    // so typing goes to the document, not the URL bar.
    focusNode.requestFocus();
  }

  Future<void> goBack() async => _session?.goBack();
  Future<void> goForward() async => _session?.goForward();
  Future<void> reload() async => _session?.reload();
  Future<void> sendPointer(MorphPointerEvent event) async =>
      _session?.sendPointer(event);
  Future<void> sendKey(MorphKeyEvent event) async => _session?.sendKey(event);
  Future<void> sendIme(MorphImeEvent event) async => _session?.sendIme(event);
  Future<String?> evaluateJavaScript(String script) async =>
      _session?.evaluateJavaScript(script);

  Future<Uint8List?> takeScreenshot() async => _session?.takeScreenshot();

  Future<void> setDesktopSite(bool enabled) async =>
      _session?.setDesktopSite(enabled);

  bool get desktopSite => _session?.desktopSite ?? false;

  Future<void> dispose() async {
    for (final sub in _subs) {
      await sub.cancel();
    }
    _subs.clear();
    await _session?.dispose();
    _session = null;
    focusNode.dispose();
    cursorShape.dispose();
    await _navController.close();
    await _pageController.close();
    await _cursorController.close();
  }
}
