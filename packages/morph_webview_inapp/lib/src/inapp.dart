import 'dart:async';
import 'dart:typed_data';
import 'dart:collection';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_inappwebview/flutter_inappwebview.dart';
import 'package:morph_webview/control.dart';
import 'package:morph_webview/morph_webview.dart';

/// macOS (WKWebView), Windows (WebView2), and Android WebView implementation of [MorphWebViewPlatform].
class MorphWebViewInApp extends MorphWebViewPlatform {
  MorphWebViewInApp();

  MorphControlClient? _client;
  StreamSubscription<MorphControlCommand>? _backendSub;
  final _sessions = <int, _InAppSession>{};
  int _nextViewId = 1;
  String? _shim;
  String? _bridgeJs;

  static void registerWith() {
    MorphWebViewPlatform.instance = MorphWebViewInApp();
  }

  Future<void> _ensureAssets() async {
    _shim ??= await rootBundle.loadString(
      'packages/morph_webview_inapp/assets/morph-rust-shim.js',
    );
    _bridgeJs ??= await rootBundle.loadString(
      'packages/morph_webview_inapp/assets/morph-bridge.js',
    );
  }

  Future<MorphControlClient> _ensureClient() async {
    final existing = _client;
    if (existing != null) {
      return existing;
    }
    final bindings = await MorphWebViewControl.ensureStarted();
    final client = MorphControlClient(bindings.endpoint);
    await client.connect();
    _client = client;
    _backendSub = bindings.backendEvents.listen(_onBackend);
    return client;
  }

  void _onBackend(MorphControlCommand command) {
    _sessions[command.viewId]?.apply(command);
  }

  @override
  Future<MorphWebViewSession> create({
    required String initialUrl,
    required int width,
    required int height,
    required double devicePixelRatio,
    String userScript = '',
  }) async {
    await _ensureAssets();
    final bindings = await MorphWebViewControl.ensureStarted();
    final client = await _ensureClient();
    final viewId = _nextViewId++;
    final session = _InAppSession(
      viewId: viewId,
      initialUrl: initialUrl,
      userScript: userScript,
      shim: _shim!,
      bridgeJs: _bridgeJs!,
      client: client,
      bindings: bindings,
      onDispose: () => _forget(viewId),
    );
    _sessions[viewId] = session;
    return session;
  }

  Future<void> _forget(int viewId) async {
    _sessions.remove(viewId);
    if (_sessions.isEmpty) {
      await _backendSub?.cancel();
      _backendSub = null;
      await _client?.shutdown();
      _client = null;
    }
  }
}

String _renderPathForHost() {
  if (Platform.isWindows) {
    return 'webview2';
  }
  if (Platform.isAndroid) {
    return 'android-webview';
  }
  return 'wkwebview';
}

class _InAppSession extends MorphWebViewSession {
  _InAppSession({
    required this.viewId,
    required this.initialUrl,
    required this.userScript,
    required this.shim,
    required this.bridgeJs,
    required this._client,
    required this._bindings,
    required this._onDispose,
  }) : renderPath = MorphRenderPathInfo(
         path: _renderPathForHost(),
         reason: 'flutter_inappwebview',
       ) {
    _view = InAppWebView(
      initialUrlRequest: URLRequest(url: WebUri(initialUrl)),
      initialSettings: InAppWebViewSettings(
        javaScriptEnabled: true,
        isInspectable: true,
        mediaPlaybackRequiresUserGesture: false,
        allowsBackForwardNavigationGestures: true,
        // WKWebView, WebView2, and Android WebView persist cookies in the
        // app's default profile. Keep every tab on that shared store so
        // logins survive view recreation and app restarts.
        incognito: false,
        cacheEnabled: true,
        sharedCookiesEnabled: true,
        thirdPartyCookiesEnabled: true,
      ),
      initialUserScripts: UnmodifiableListView<UserScript>([
        UserScript(
          source: shim,
          injectionTime: UserScriptInjectionTime.AT_DOCUMENT_START,
          forMainFrameOnly: false,
        ),
        UserScript(
          source: bridgeJs,
          injectionTime: UserScriptInjectionTime.AT_DOCUMENT_START,
          forMainFrameOnly: false,
        ),
        if (userScript.isNotEmpty)
          UserScript(
            source: userScript,
            injectionTime: UserScriptInjectionTime.AT_DOCUMENT_START,
            forMainFrameOnly: false,
          ),
      ]),
      onWebViewCreated: (controller) {
        _controller = controller;
        controller.addJavaScriptHandler(
          handlerName: 'morphRust',
          callback: (args) {
            final json = args.isEmpty ? '{}' : args.first.toString();
            unawaited(_bindings.emitPageMessage(viewId: viewId, json: json));
            return null;
          },
        );
        _flushPending(controller);
      },
      onLoadStart: (controller, url) {
        unawaited(_emitNav(controller, url, isLoading: true));
      },
      onLoadStop: (controller, url) {
        unawaited(_emitNav(controller, url, isLoading: false, progress: 1));
      },
      onProgressChanged: (controller, progress) {
        unawaited(
          _emitNav(
            controller,
            null,
            isLoading: progress < 100,
            progress: progress / 100,
          ),
        );
      },
      onTitleChanged: (controller, title) {
        unawaited(_emitNav(controller, null, title: title));
      },
    );
    _navSub = _client.navigation.listen((event) {
      if (event.$1 == viewId) {
        _navController.add(event.$2);
      }
    });
    _pageSub = _client.pageMessages.listen((event) {
      if (event.$1 == viewId) {
        _pageController.add(event.$2);
      }
    });
  }

  final int viewId;
  final String initialUrl;
  final String userScript;
  final String shim;
  final String bridgeJs;
  final MorphControlClient _client;
  final MorphControlBindings _bindings;
  final Future<void> Function() _onDispose;
  late final InAppWebView _view;
  InAppWebViewController? _controller;
  final _pending = <MorphControlCommand>[];
  final _navController = StreamController<MorphNavigationEvent>.broadcast();
  final _pageController = StreamController<String>.broadcast();
  StreamSubscription<(int, MorphNavigationEvent)>? _navSub;
  StreamSubscription<(int, String)>? _pageSub;

  @override
  int? get textureId => null;

  @override
  Widget get platformView => _view;

  @override
  final MorphRenderPathInfo renderPath;

  @override
  bool get handlesPageBridge => true;

  @override
  Stream<MorphNavigationEvent> get navigation => _navController.stream;

  @override
  Stream<String> get pageMessages => _pageController.stream;

  Future<void> _emitNav(
    InAppWebViewController controller,
    WebUri? url, {
    String? title,
    bool? isLoading,
    double? progress,
  }) async {
    final current = url ?? await controller.getUrl();
    final resolvedTitle = title ?? await controller.getTitle() ?? '';
    final canGoBack = await controller.canGoBack();
    final canGoForward = await controller.canGoForward();
    await _bindings.emitNavigation(
      viewId: viewId,
      url: current?.toString() ?? '',
      title: resolvedTitle,
      canGoBack: canGoBack,
      canGoForward: canGoForward,
      isLoading: isLoading ?? false,
      progress: progress ?? 0,
    );
  }

  void apply(MorphControlCommand command) {
    final controller = _controller;
    if (controller == null) {
      _pending.add(command);
      return;
    }
    unawaited(_apply(controller, command));
  }

  void _flushPending(InAppWebViewController controller) {
    final queued = List<MorphControlCommand>.of(_pending);
    _pending.clear();
    for (final command in queued) {
      unawaited(_apply(controller, command));
    }
  }

  Future<void> _apply(
    InAppWebViewController controller,
    MorphControlCommand command,
  ) async {
    switch (command.kind) {
      case 'navigate':
        await controller.loadUrl(
          urlRequest: URLRequest(url: WebUri(command.url)),
        );
      case 'go_back':
        if (await controller.canGoBack()) {
          await controller.goBack();
        }
      case 'go_forward':
        if (await controller.canGoForward()) {
          await controller.goForward();
        }
      case 'reload':
        await controller.reload();
      case 'evaluate_js':
        try {
          final value = await controller.evaluateJavascript(
            source: command.script,
          );
          if (command.requestId != 0) {
            await _bindings.completeEvaluateJs(
              viewId: viewId,
              requestId: BigInt.from(command.requestId),
              result: value?.toString() ?? '',
              error: '',
            );
          }
        } catch (error) {
          if (command.requestId != 0) {
            await _bindings.completeEvaluateJs(
              viewId: viewId,
              requestId: BigInt.from(command.requestId),
              result: '',
              error: error.toString(),
            );
          }
        }
      default:
        // Pointer, key, and IME events are handled natively by the platform view.
        break;
    }
  }

  @override
  Future<void> resize({
    required int width,
    required int height,
    required double devicePixelRatio,
  }) async {}

  @override
  Future<void> loadUrl(String url) async {
    _client.send(
      viewId: viewId,
      navigate: Navigate(url: url),
    );
  }

  @override
  Future<void> goBack() async {
    _client.send(viewId: viewId, goBack: GoBack());
  }

  @override
  Future<void> goForward() async {
    _client.send(viewId: viewId, goForward: GoForward());
  }

  @override
  Future<void> reload() async {
    _client.send(viewId: viewId, reload: Reload());
  }

  @override
  Future<void> sendPointer(MorphPointerEvent event) async {}

  @override
  Future<void> sendKey(MorphKeyEvent event) async {}

  @override
  Future<String?> evaluateJavaScript(String script) {
    return _client.evaluateJavaScript(viewId, script);
  }

  bool _desktopSite = false;

  @override
  bool get desktopSite => _desktopSite;

  @override
  Future<void> setDesktopSite(bool enabled) async {
    _desktopSite = enabled;
    final controller = _controller;
    if (controller == null) {
      return;
    }
    await controller.setSettings(
      settings: InAppWebViewSettings(
        preferredContentMode: enabled
            ? UserPreferredContentMode.DESKTOP
            : UserPreferredContentMode.MOBILE,
        useWideViewPort: enabled,
        loadWithOverviewMode: enabled,
      ),
    );
    await controller.reload();
  }

  @override
  Future<Uint8List?> takeScreenshot() async {
    return _controller?.takeScreenshot();
  }

  @override
  Future<void> dispose() async {
    await _navSub?.cancel();
    await _pageSub?.cancel();
    await _navController.close();
    await _pageController.close();
    await _onDispose();
  }
}
