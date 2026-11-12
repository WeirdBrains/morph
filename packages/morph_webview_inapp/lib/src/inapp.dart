import 'dart:async';
import 'dart:collection';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_inappwebview/flutter_inappwebview.dart';
import 'package:flutter_inappwebview_platform_interface/flutter_inappwebview_platform_interface.dart'
    show InternalInAppWebViewKeepAlive;
import 'package:morph_webview/control.dart';
import 'package:morph_webview/morph_webview.dart';

import 'load_errors.dart';
import 'popups.dart';
import 'profile_store.dart';
import 'renderer_watch.dart';
import 'settings.dart';

/// macOS (WKWebView), Windows (WebView2), and Android WebView implementation of [MorphWebViewPlatform].
class MorphWebViewInApp extends MorphWebViewPlatform {
  MorphWebViewInApp();

  Future<MorphControlClient>? _client;
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

  Future<MorphControlClient> _ensureClient() {
    // Single-flight: tabs restored together must share one client.
    return _client ??= _connectClient();
  }

  Future<MorphControlClient> _connectClient() async {
    try {
      final bindings = await MorphWebViewControl.ensureStarted();
      final client = MorphControlClient(bindings.endpoint);
      await client.connect();
      _backendSub ??= bindings.backendEvents.listen(_onBackend);
      return client;
    } catch (_) {
      _client = null;
      rethrow;
    }
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
    String profileId = 'default',
  }) async {
    await _ensureAssets();
    final bindings = await MorphWebViewControl.ensureStarted();
    final client = await _ensureClient();
    final viewId = _nextViewId++;
    await morphProfileStore.prepareMacos(profileId);
    final environment = await morphProfileStore.windowsEnvironment(profileId);
    final android = await _AndroidView.prepare(viewId, profileId);
    final session = _InAppSession(
      viewId: viewId,
      initialUrl: initialUrl,
      userScript: userScript,
      profileId: profileId,
      shim: _shim!,
      bridgeJs: _bridgeJs!,
      client: client,
      bindings: bindings,
      onDispose: () => _forget(viewId),
      webViewEnvironment: environment,
      android: android,
    );
    _sessions[viewId] = session;
    return session;
  }

  @override
  Future<void> deleteProfileData(String profileId) async {
    await morphProfileStore.deleteAndroid(profileId);
  }

  @override
  Future<void> pruneProfileData(Iterable<String> liveProfileIds) async {
    await morphProfileStore.pruneAndroid(liveProfileIds);
  }

  Future<void> _forget(int viewId) async {
    // The control client and backend subscription stay up for the app's
    // life. Tearing them down when the last tab closed raced the next
    // create() (profile switches dispose every tab first), and the backend
    // event stream cannot be listened to twice.
    _sessions.remove(viewId);
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

String _renderReasonForHost(String profileId, {required bool isolated}) {
  if (Platform.isWindows) {
    return 'webview2 userDataFolder profiles/$profileId/webview2';
  }
  if (Platform.isAndroid) {
    return isolated
        ? 'androidx.webkit profile ${morphAndroidProfileName(profileId)}'
        : 'android WebView lacks MULTI_PROFILE; profile $profileId blocked';
  }
  return 'wkwebview; per-profile WKWebsiteDataStore on macOS 14+';
}

/// One native Android WebView bound to a Morph profile.
///
/// The keep-alive id names the native WebView. MainActivity looks it up when
/// the WebView is constructed and gives it the profile's androidx.webkit
/// Profile before anything else touches it. Every native view (including a
/// surface rebuild) gets a fresh id, so a rebuild never reuses a stale view.
class _AndroidView {
  _AndroidView(this.keepAlive, {required this.isolated});

  final InAppWebViewKeepAlive keepAlive;

  /// False when this device cannot keep the profile's storage apart.
  final bool isolated;

  static Future<_AndroidView?> prepare(
    int viewId,
    String profileId, {
    String kind = 'view',
  }) async {
    if (!Platform.isAndroid) {
      return null;
    }
    final keepAlive = InAppWebViewKeepAlive()
      ..id = 'morph-$kind-$viewId-${DateTime.now().microsecondsSinceEpoch}';
    final reply = await morphProfileStore.prepareAndroid(
      profileId,
      keepAlive.id,
    );
    return _AndroidView(keepAlive, isolated: reply?.isolated ?? false);
  }

  Future<void> dispose() async {
    await morphProfileStore.forgetAndroidView(keepAlive.id);
    try {
      await InAppWebViewController.disposeKeepAlive(keepAlive);
    } catch (_) {
      // Already gone with the engine.
    }
  }
}

class _InAppSession extends MorphWebViewSession {
  _InAppSession({
    required this.viewId,
    required this.initialUrl,
    required this.userScript,
    required this.profileId,
    required this.shim,
    required this.bridgeJs,
    required this._client,
    required this._bindings,
    required this._onDispose,
    this.webViewEnvironment,
    _AndroidView? android,
  }) : assert(profileId.isNotEmpty),
       _android = android,
       isolated = android?.isolated ?? true,
       renderPath = MorphRenderPathInfo(
         path: _renderPathForHost(),
         reason: _renderReasonForHost(
           profileId,
           isolated: android?.isolated ?? true,
         ),
       ) {
    _view = _buildView(initialUrl);
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

  InAppWebView _buildView(String url, {int generation = 0}) {
    return InAppWebView(
      key: ValueKey<int>(generation),
      keepAlive: _android?.keepAlive,
      // Never load a site into shared storage for a non-default profile.
      initialUrlRequest: isolated ? URLRequest(url: WebUri(url)) : null,
      initialData: isolated
          ? null
          : InAppWebViewInitialData(data: morphAndroidUnisolatedHtml),
      webViewEnvironment: webViewEnvironment,
      initialSettings: _settings(),
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
        if (_buffering == MorphBuffering.idle && Platform.isAndroid) {
          // A view rebuilt while its tab is hidden stays paused.
          unawaited(controller.pause());
        }
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
        _rememberUrl(url);
        unawaited(_emitNav(controller, url, isLoading: true));
      },
      onReceivedError: (controller, request, error) {
        final failed = morphInAppLoadError(
          url: request.url.toString(),
          isForMainFrame: request.isForMainFrame ?? true,
          type: error.type,
          description: error.description,
        );
        if (failed != null) {
          _loadErrors.add(failed);
        }
      },
      // Android only: there this fires just for certificate errors, and the
      // page is left blank after the (default) cancel. Apple platforms ask
      // here for every TLS connection, so they keep the default handling.
      onReceivedServerTrustAuthRequest: Platform.isAndroid
          ? (controller, challenge) async {
              final url = morphCertificateErrorUrl(
                challenge.protectionSpace.host,
                [_requestedUrl, _lastUrl, initialUrl],
              );
              if (url != null) {
                _loadErrors.add(
                  MorphLoadError(
                    url: url,
                    kind: MorphLoadErrorKind.certificate,
                    message: challenge.protectionSpace.sslError?.message ?? '',
                  ),
                );
              }
              return ServerTrustAuthResponse(
                action: ServerTrustAuthResponseAction.CANCEL,
              );
            }
          : null,
      onLoadStop: (controller, url) {
        _rememberUrl(url);
        unawaited(_emitNav(controller, url, isLoading: false, progress: 1));
      },
      onUpdateVisitedHistory: (controller, url, isReload) {
        _rememberUrl(url);
      },
      onCreateWindow: (controller, action) => _openPopup(action),
      onRenderProcessGone: (controller, detail) {
        // The native WebView is unusable once its renderer is gone. The tab
        // shows its crash state; recoverRenderer builds a fresh view. On
        // Android every WebView in the app shares one renderer, so every
        // tab gets this at once; the shell itself keeps running.
        _watch.renderProcessGone(didCrash: detail.didCrash);
      },
      onRenderProcessUnresponsive: (controller, url) async {
        // Asked again every few seconds while hung; ended once the user
        // chose to reload.
        return _watch.renderProcessUnresponsive()
            ? WebViewRenderProcessAction.TERMINATE
            : null;
      },
      onRenderProcessResponsive: (controller, url) async {
        _watch.renderProcessResponsive();
        return null;
      },
      onWebContentProcessDidTerminate: (controller) {
        _watch.webContentProcessTerminated();
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
  }

  final int viewId;
  final String initialUrl;
  final String userScript;
  final String profileId;
  final String shim;
  final String bridgeJs;
  final MorphControlClient _client;
  final MorphControlBindings _bindings;
  final Future<void> Function() _onDispose;
  final WebViewEnvironment? webViewEnvironment;
  _AndroidView? _android;

  /// False when this profile's storage cannot be kept apart on this device;
  /// navigation commands are then ignored.
  final bool isolated;
  late InAppWebView _view;
  final _generation = ValueNotifier<int>(0);
  String? _lastUrl;
  final _watch = InAppRendererWatch();
  bool get _rendererGone => _watch.gone;
  final _surfaceLost = StreamController<void>.broadcast();
  InAppWebViewController? _controller;
  final _pending = <MorphControlCommand>[];
  final _navController = StreamController<MorphNavigationEvent>.broadcast();
  final _pageController = StreamController<String>.broadcast();
  final _loadErrors = StreamController<MorphLoadError>.broadcast();

  /// The last address the shell asked for, which may not have committed.
  String? _requestedUrl;

  @override
  Stream<MorphLoadError> get loadErrors => _loadErrors.stream;
  StreamSubscription<(int, MorphNavigationEvent)>? _navSub;
  StreamSubscription<(int, String)>? _pageSub;

  @override
  int? get textureId => null;

  /// Stable wrapper so the shell never sees the widget change; a rebuild swaps
  /// the keyed [InAppWebView] inside it for a fresh native view.
  @override
  late final Widget platformView = MorphPopupOverlay(
    stack: _popups,
    onClose: (windowId) => unawaited(_closePopup(windowId)),
    child: ValueListenableBuilder<int>(
      valueListenable: _generation,
      builder: (context, generation, _) => _view,
    ),
  );

  final _popups = MorphPopupStack();

  /// Native views (keep-alives) behind open popups, by window id.
  final _popupViews = <int, _AndroidView>{};

  /// A page asked for a new window: give the engine a real web view for it,
  /// in this tab's profile, stacked over the tab.
  Future<bool> _openPopup(CreateWindowAction action) async {
    if (!isolated || _popups.isFull) {
      return false;
    }
    final windowId = action.windowId;
    final android = await _AndroidView.prepare(
      viewId,
      profileId,
      kind: 'popup',
    );
    if (android != null) {
      _popupViews[windowId] = android;
    }
    late final MorphPopup popup;
    popup = MorphPopup(
      windowId: windowId,
      url: action.request.url?.toString() ?? '',
      view: InAppWebView(
        key: ValueKey<String>('morph-popup-$windowId'),
        windowId: windowId,
        keepAlive: android?.keepAlive,
        webViewEnvironment: webViewEnvironment,
        initialSettings: morphInAppWebViewSettings(),
        onCreateWindow: (controller, next) => _openPopup(next),
        onCloseWindow: (controller) => unawaited(_closePopup(windowId)),
        onLoadStart: (controller, url) {
          popup.address.value = url?.toString() ?? popup.address.value;
        },
        onLoadStop: (controller, url) {
          popup.address.value = url?.toString() ?? popup.address.value;
        },
        onTitleChanged: (controller, title) {
          popup.title.value = title ?? '';
        },
        onRenderProcessGone: (controller, detail) {
          _watch.renderProcessGone(didCrash: detail.didCrash);
        },
      ),
    );
    if (!_popups.push(popup)) {
      _popupViews.remove(windowId);
      await android?.dispose();
      popup.dispose();
      return false;
    }
    return true;
  }

  Future<void> _closePopup(int windowId) async {
    final popup = _popups.remove(windowId);
    final android = _popupViews.remove(windowId);
    // Let the frame that drops the popup's widget go first.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      popup?.dispose();
      if (android != null) {
        unawaited(android.dispose());
      }
    });
  }

  Future<void> _closeAllPopups() async {
    for (final popup in _popups.clear()) {
      popup.dispose();
      await _popupViews.remove(popup.windowId)?.dispose();
    }
  }

  void _rememberUrl(WebUri? url) {
    final value = url?.toString() ?? '';
    if (value.isNotEmpty && value != 'about:blank') {
      _lastUrl = value;
    }
  }

  @override
  Stream<void> get surfaceLost => _surfaceLost.stream;

  @override
  Stream<MorphRendererEvent> get renderer => _watch.events;

  @override
  Future<void> terminateRenderer() => _watch.terminate();

  @override
  Future<void> recoverRenderer() async {
    await rebuildSurface();
  }

  @override
  Future<void> repaintSurface() async {
    final controller = _controller;
    if (controller == null || _rendererGone) {
      return;
    }
    try {
      await controller
          .evaluateJavascript(source: morphSurfaceRepaintScript)
          .timeout(const Duration(seconds: 2));
    } catch (_) {
      // A page that is still navigating cannot run script; nothing to nudge.
    }
  }

  @override
  Future<MorphSurfaceVerdict> checkSurface() async {
    if (_rendererGone) {
      return MorphSurfaceVerdict.rendererGone;
    }
    final controller = _controller;
    if (controller == null) {
      return MorphSurfaceVerdict.unknown;
    }
    try {
      final probe = MorphSurfaceProbe.tryParse(
        await controller
            .evaluateJavascript(source: morphSurfaceProbeScript)
            .timeout(const Duration(seconds: 2)),
      );
      if (probe == null || !probe.hasVisibleContent) {
        return MorphSurfaceVerdict.unknown;
      }
      final png = await controller
          .takeScreenshot(
            screenshotConfiguration: ScreenshotConfiguration(
              snapshotWidth: 270,
              compressFormat: CompressFormat.PNG,
            ),
          )
          .timeout(const Duration(seconds: 3));
      return morphClassifySurface(
        probe: probe,
        frameIsFlat: png == null ? null : await _frameIsFlat(png),
      );
    } catch (_) {
      return MorphSurfaceVerdict.unknown;
    }
  }

  static Future<bool?> _frameIsFlat(Uint8List png) async {
    final codec = await ui.instantiateImageCodec(png);
    try {
      final frame = await codec.getNextFrame();
      final image = frame.image;
      try {
        final data = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
        return data == null
            ? null
            : morphFrameIsFlat(data.buffer.asUint8List());
      } finally {
        image.dispose();
      }
    } finally {
      codec.dispose();
    }
  }

  @override
  Future<bool> rebuildSurface() async {
    // Commands wait for the new native view; the old one is disposed when
    // its keyed widget leaves the tree.
    _controller = null;
    _watch.rebuilt();
    // A new main view has no tie to the old one's windows.
    await _closeAllPopups();
    final url = _lastUrl ?? initialUrl;
    final next = _generation.value + 1;
    // A keep-alive view outlives its widget, so give the rebuild its own
    // native view (prepared for this profile) and drop the old one.
    final previous = _android;
    if (previous != null) {
      _android = await _AndroidView.prepare(viewId, profileId);
    }
    _view = _buildView(url, generation: next);
    _generation.value = next;
    if (previous != null) {
      WidgetsBinding.instance.addPostFrameCallback(
        (_) => unawaited(previous.dispose()),
      );
    }
    return true;
  }

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
    if (!isolated && command.kind == 'navigate') {
      return;
    }
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
    _requestedUrl = url;
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

  /// The desktop-site choice once the user made one (null: platform default).
  bool? _desktopSiteChoice;

  MorphBuffering _buffering = MorphBuffering.full;

  InAppWebViewSettings _settings() => morphInAppWebViewSettings(
    desktopSite: _desktopSiteChoice,
    idle: _buffering == MorphBuffering.idle,
  );

  @override
  Future<void> setDesktopSite(bool enabled) async {
    _desktopSite = enabled;
    _desktopSiteChoice = enabled;
    final controller = _controller;
    if (controller == null) {
      return;
    }
    await controller.setSettings(settings: _settings());
    await controller.reload();
  }

  @override
  Future<void> setBuffering(MorphBuffering buffering) async {
    if (_buffering == buffering) {
      return;
    }
    _buffering = buffering;
    await _applyBuffering();
  }

  Future<void> _applyBuffering() async {
    final controller = _controller;
    if (controller == null || !Platform.isAndroid) {
      return;
    }
    try {
      await controller.setSettings(settings: _settings());
      if (_buffering == MorphBuffering.idle) {
        // WebView.onPause: the page counts as hidden, its compositor lets
        // go of tiles and animations stop. Timers keep running (only the
        // process-wide pauseTimers stops those), so background work such as
        // music players carries on.
        await controller.pause();
      } else {
        await controller.resume();
      }
    } on Exception {
      // The view is going away; the next one starts from _settings().
    }
  }

  @override
  Future<Uint8List?> takeScreenshot() async {
    return _controller?.takeScreenshot();
  }

  @override
  Future<void> dispose() async {
    await _closeAllPopups();
    _popups.dispose();
    await _surfaceLost.close();
    await _watch.close();
    _generation.dispose();
    await _navSub?.cancel();
    await _pageSub?.cancel();
    await _navController.close();
    await _pageController.close();
    await _loadErrors.close();
    await _android?.dispose();
    await _onDispose();
  }
}
