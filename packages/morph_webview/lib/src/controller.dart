import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/widgets.dart';

import 'events.dart';
import 'platform.dart';
import 'surface_health.dart';

/// Public handle used by the Morph shell and by widget tests (via a fake session).
class MorphWebViewController {
  MorphWebViewController({
    this.initialUrl = 'https://example.com',
    this.surfaceSettle = const Duration(milliseconds: 600),
    this.rebuildCooldown = const Duration(seconds: 20),
    DateTime Function()? clock,
  }) : _clock = clock ?? DateTime.now;

  final String initialUrl;

  /// How long a rebuilt surface gets to paint before it is checked again.
  final Duration surfaceSettle;

  /// Minimum gap between two automatic rebuilds, so a page that really is a
  /// flat colour can never put the view into a rebuild loop.
  final Duration rebuildCooldown;

  final DateTime Function() _clock;
  Future<void>? _creating;
  Future<MorphSurfaceVerdict>? _healing;
  DateTime? _lastRebuild;
  int _surfaceRebuilds = 0;

  /// How many times this view rebuilt its native surface to recover from a
  /// blank or lost surface.
  int get surfaceRebuilds => _surfaceRebuilds;

  MorphWebViewSession? _session;
  String? _pendingUrl;
  MorphNavigationEvent _navigation = const MorphNavigationEvent(url: '');
  MorphRenderPathInfo _renderPath = const MorphRenderPathInfo(
    path: 'uninitialized',
    reason: 'not created',
  );
  final focusNode = FocusNode(debugLabel: 'morph-webview');
  final cursorShape = ValueNotifier<String>('default');

  /// Set while this tab's renderer is crashed, killed or hung. Only this tab
  /// is affected; [recoverFromFault] brings it back.
  final rendererFault = ValueNotifier<MorphRendererFault?>(null);
  Future<void>? _recovering;
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
    // Layout can ask again (keyboard, sheet, rotation) while the first
    // create is still in flight. Creating twice would leave two native views
    // racing for this tab, so later callers wait for the first one.
    final inFlight = _creating;
    if (inFlight != null) {
      await inFlight;
      if (_session != null) {
        await resize(
          width: width,
          height: height,
          devicePixelRatio: devicePixelRatio,
        );
      }
      return;
    }
    final creating = _create(
      width: width,
      height: height,
      devicePixelRatio: devicePixelRatio,
      userScript: userScript,
      profileId: profileId,
    );
    _creating = creating;
    try {
      await creating;
    } finally {
      if (identical(_creating, creating)) {
        _creating = null;
      }
    }
  }

  Future<void> _create({
    required int width,
    required int height,
    required double devicePixelRatio,
    required String userScript,
    required String profileId,
  }) async {
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
    _subs.add(
      session.surfaceLost.listen((_) => unawaited(_rebuild(force: true))),
    );
    _subs.add(session.renderer.listen(_onRenderer));
    final pending = _pendingUrl;
    if (pending != null) {
      // An address submitted while create was in flight.
      _pendingUrl = null;
      await session.loadUrl(pending);
    }
  }

  void _onRenderer(MorphRendererEvent event) {
    final fault = event.fault;
    if (fault == null) {
      if (rendererFault.value == MorphRendererFault.unresponsive) {
        rendererFault.value = null;
      }
      return;
    }
    // A hang report never hides a crash that already happened.
    if (fault == MorphRendererFault.unresponsive &&
        rendererFault.value != null &&
        rendererFault.value != MorphRendererFault.unresponsive) {
      return;
    }
    rendererFault.value = fault;
  }

  /// Reloads a tab whose renderer crashed, was killed or hung: ends a hung
  /// renderer first, then brings the page back in a new one. Concurrent
  /// calls share one recovery.
  Future<void> recoverFromFault() {
    final running = _recovering;
    if (running != null) {
      return running;
    }
    final recovering = _recover();
    _recovering = recovering;
    return recovering.whenComplete(() {
      if (identical(_recovering, recovering)) {
        _recovering = null;
      }
    });
  }

  Future<void> _recover() async {
    final session = _session;
    final fault = rendererFault.value;
    if (session == null || fault == null) {
      return;
    }
    if (fault == MorphRendererFault.unresponsive) {
      await session.terminateRenderer();
    }
    rendererFault.value = null;
    await session.recoverRenderer();
  }

  /// Makes sure the visible surface is painting the page, and recovers it
  /// when it is not: first a repaint nudge, then (if the page has content but
  /// the frame is still flat) a fresh native view on the same URL.
  ///
  /// Concurrent calls share one check. Call it after anything that can leave
  /// a stale surface: the keyboard or a sheet closing, the app resuming,
  /// returning from the tab grid, a tab or profile switch, a finished load.
  Future<MorphSurfaceVerdict> ensureSurfaceHealthy() {
    final running = _healing;
    if (running != null) {
      return running;
    }
    final heal = _heal();
    _healing = heal;
    return heal.whenComplete(() {
      if (identical(_healing, heal)) {
        _healing = null;
      }
    });
  }

  Future<MorphSurfaceVerdict> _heal() async {
    final session = _session;
    if (session == null) {
      return MorphSurfaceVerdict.unknown;
    }
    if (rendererFault.value != null) {
      // A crashed or hung tab waits for the user (or the shell) to recover
      // it; rebuilding here would hide the crash and could loop on a page
      // that crashes on load.
      return MorphSurfaceVerdict.rendererGone;
    }
    await session.repaintSurface();
    var verdict = await session.checkSurface();
    if (verdict == MorphSurfaceVerdict.blank) {
      // Confirm before rebuilding: nudge again, give it a moment, look again.
      // Only a surface that stays flat while the page has content is rebuilt.
      await session.repaintSurface();
      await Future<void>.delayed(surfaceSettle);
      if (!identical(_session, session)) {
        return MorphSurfaceVerdict.unknown;
      }
      verdict = await session.checkSurface();
    }
    if (verdict != MorphSurfaceVerdict.blank &&
        verdict != MorphSurfaceVerdict.rendererGone) {
      return verdict;
    }
    if (!await _rebuild(force: verdict == MorphSurfaceVerdict.rendererGone)) {
      return verdict;
    }
    await Future<void>.delayed(surfaceSettle);
    if (!identical(_session, session)) {
      return MorphSurfaceVerdict.unknown;
    }
    return session.checkSurface();
  }

  Future<bool> _rebuild({required bool force}) async {
    final session = _session;
    if (session == null) {
      return false;
    }
    final now = _clock();
    final last = _lastRebuild;
    // A dead renderer is rebuilt right away, but never more than once every
    // couple of seconds so a page that crashes it on load cannot spin.
    final gap = force ? const Duration(seconds: 2) : rebuildCooldown;
    if (last != null && now.difference(last) < gap) {
      return false;
    }
    final rebuilt = await session.rebuildSurface();
    if (rebuilt) {
      _lastRebuild = now;
      _surfaceRebuilds++;
    }
    return rebuilt;
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
      // attaches (common on Android while the system WebView starts). If a
      // create is already in flight it loads this once it lands.
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
    rendererFault.dispose();
    await _navController.close();
    await _pageController.close();
    await _cursorController.close();
  }
}
