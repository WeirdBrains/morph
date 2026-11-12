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
    this.unresponsiveGrace = defaultUnresponsiveGrace,
    this.idleAfter = defaultIdleAfter,
    DateTime Function()? clock,
  }) : _clock = clock ?? DateTime.now;

  /// A controller for a window [session]'s page opened (window.open). It is
  /// already attached; [MorphWebView] only sizes it.
  MorphWebViewController._popup(
    MorphWebViewSession session, {
    required MorphWebViewController opener,
  }) : initialUrl = '',
       surfaceSettle = opener.surfaceSettle,
       rebuildCooldown = opener.rebuildCooldown,
       unresponsiveGrace = opener.unresponsiveGrace,
       idleAfter = opener.idleAfter,
       _clock = opener._clock,
       _opener = opener {
    _adopt(session);
  }

  /// Most popups stacked over one tab, the same cap as Android. Further
  /// window.open calls are refused (the page sees a closed window).
  static const maxPopups = 4;

  /// How long a renderer may stay unresponsive before the tab says so.
  /// Heavy pages (a YouTube watch page starting its player) routinely block
  /// the renderer for several seconds; WebKit flags them after about three.
  static const defaultUnresponsiveGrace = Duration(seconds: 10);

  /// How long a tab stays hidden before it gives up its extra buffering.
  static const defaultIdleAfter = Duration(seconds: 45);

  /// How long this tab stays hidden before [MorphBuffering.idle].
  final Duration idleAfter;

  final String initialUrl;

  /// How long a rebuilt surface gets to paint before it is checked again.
  final Duration surfaceSettle;

  /// Minimum gap between two automatic rebuilds, so a page that really is a
  /// flat colour can never put the view into a rebuild loop.
  final Duration rebuildCooldown;

  final DateTime Function() _clock;

  /// See [defaultUnresponsiveGrace]. Zero reports a hang at once.
  final Duration unresponsiveGrace;
  Timer? _hangTimer;
  MorphWebViewController? _opener;

  /// Windows this tab's pages opened, oldest first; the last one is on top.
  /// Each is a full controller for its own page.
  final popups = ValueNotifier<List<MorphWebViewController>>(const []);

  /// The failed main-frame load this tab is showing, if any. Cleared by the
  /// next navigation.
  final loadError = ValueNotifier<MorphLoadError?>(null);
  Future<void>? _creating;
  Future<MorphSurfaceVerdict>? _healing;
  Future<MorphSurfaceVerdict>? _healQueued;
  DateTime? _lastRebuild;
  int _surfaceRebuilds = 0;

  /// How many times this view rebuilt its native surface to recover from a
  /// blank or lost surface.
  int get surfaceRebuilds => _surfaceRebuilds;

  bool _visible = true;
  Timer? _idleTimer;
  MorphBuffering _buffering = MorphBuffering.full;

  /// Whether the shell shows this tab.
  bool get visible => _visible;

  /// What the engine is asked to keep rendered for this tab.
  MorphBuffering get buffering => _buffering;

  /// The shell shows (true) or hides (false) this tab. Showing restores full
  /// buffering at once (call it as a tab switch starts, so the renderer is
  /// warming up while the switch animates); hiding drops to
  /// [MorphBuffering.idle] only after [idleAfter], so flipping between two
  /// tabs keeps both fast.
  void setVisible(bool visible) {
    if (visible) {
      _idleTimer?.cancel();
      _idleTimer = null;
      _visible = true;
      _setBuffering(MorphBuffering.full);
      return;
    }
    if (!_visible) {
      return;
    }
    _visible = false;
    _idleTimer?.cancel();
    _idleTimer = Timer(idleAfter, () {
      _idleTimer = null;
      if (!_visible) {
        _setBuffering(MorphBuffering.idle);
      }
    });
  }

  void _setBuffering(MorphBuffering buffering) {
    if (_buffering == buffering) {
      return;
    }
    _buffering = buffering;
    final session = _session;
    if (session != null) {
      unawaited(session.setBuffering(buffering));
    }
  }

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

  /// What the backend knows about the last fault (say, missing video
  /// decoders); empty when it has nothing to add.
  String rendererDetail = '';
  Future<void>? _recovering;
  bool _terminating = false;
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
    _adopt(session);
    final pending = _pendingUrl;
    if (pending != null) {
      // An address submitted while create was in flight.
      _pendingUrl = null;
      await session.loadUrl(pending);
    }
  }

  void _adopt(MorphWebViewSession session) {
    _session = session;
    _renderPath = session.renderPath;
    if (_buffering != MorphBuffering.full) {
      // Hidden long enough before its view existed.
      unawaited(session.setBuffering(_buffering));
    }
    _subs.add(
      session.navigation.listen((event) {
        _navigation = event;
        final error = loadError.value;
        if (error != null && event.isLoading && event.url != error.url) {
          loadError.value = null;
        }
        _navController.add(event);
      }),
    );
    _subs.add(session.loadErrors.listen((error) => loadError.value = error));
    _subs.add(
      session.popups.listen((popup) => (_opener ?? this)._addPopup(popup)),
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
  }

  void _addPopup(MorphWebViewSession session) {
    if (_disposed || popups.value.length >= maxPopups) {
      unawaited(session.dispose());
      return;
    }
    final popup = MorphWebViewController._popup(session, opener: this);
    popup._subs.add(
      session.closeRequested.listen((_) => unawaited(closePopup(popup))),
    );
    popups.value = [...popups.value, popup];
  }

  /// Closes [popup] (its close button or window.close()) and returns to the
  /// window beneath it.
  Future<void> closePopup(MorphWebViewController popup) async {
    final list = popups.value;
    if (!list.contains(popup)) {
      return;
    }
    popups.value = [
      for (final p in list)
        if (!identical(p, popup)) p,
    ];
    await popup.dispose();
  }

  void _onRenderer(MorphRendererEvent event) {
    if (_terminating) {
      // The renderer is ending because the user chose to reload.
      return;
    }
    final fault = event.fault;
    if (fault != MorphRendererFault.unresponsive) {
      _hangTimer?.cancel();
      _hangTimer = null;
    }
    if (fault == MorphRendererFault.unresponsive &&
        rendererFault.value == null &&
        unresponsiveGrace > Duration.zero) {
      // Only a hang that outlasts the grace is shown; a report that clears
      // before then never reaches the user.
      _hangTimer ??= Timer(unresponsiveGrace, () {
        _hangTimer = null;
        if (!_disposed && rendererFault.value == null) {
          rendererFault.value = MorphRendererFault.unresponsive;
        }
      });
      return;
    }
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
    rendererDetail = event.detail;
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
      _terminating = true;
      try {
        await session.terminateRenderer();
        // Let the termination's own events arrive before listening again.
        await Future<void>.delayed(Duration.zero);
      } finally {
        _terminating = false;
      }
    }
    rendererFault.value = null;
    await session.recoverRenderer();
  }

  /// Makes sure the visible surface is painting the page, and recovers it
  /// when it is not: first a repaint nudge, then (if the page has content but
  /// the frame is still flat) a fresh native view on the same URL.
  ///
  /// Calls made while a check runs share one follow-up check. Call it after
  /// anything that can leave a stale surface: the keyboard or a sheet
  /// closing, the app resuming, returning from the tab grid, a tab or profile
  /// switch, a finished load.
  Future<MorphSurfaceVerdict> ensureSurfaceHealthy() {
    final running = _healing;
    if (running == null) {
      return _startHeal();
    }
    // A check asked for while one is running (say, the load that a sheet's
    // close check raced with just finished) must still look at the page as
    // it is now; the running check may have probed it mid-navigation. Queue
    // one follow-up that every later caller shares.
    return _healQueued ??= running.then(
      (_) => _runQueuedHeal(),
      onError: (Object _) => _runQueuedHeal(),
    );
  }

  Future<MorphSurfaceVerdict> _runQueuedHeal() {
    _healQueued = null;
    return _startHeal();
  }

  Future<MorphSurfaceVerdict> _startHeal() {
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
    loadError.value = null;
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

  Future<void> goBack() async {
    loadError.value = null;
    await _session?.goBack();
  }

  Future<void> goForward() async {
    loadError.value = null;
    await _session?.goForward();
  }

  Future<void> reload() async {
    loadError.value = null;
    await _session?.reload();
  }

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

  bool _disposed = false;

  Future<void> dispose() async {
    _disposed = true;
    _hangTimer?.cancel();
    final open = popups.value;
    popups.value = const [];
    for (final popup in open) {
      await popup.dispose();
    }
    _idleTimer?.cancel();
    _idleTimer = null;
    for (final sub in _subs) {
      await sub.cancel();
    }
    _subs.clear();
    await _session?.dispose();
    _session = null;
    focusNode.dispose();
    cursorShape.dispose();
    rendererFault.dispose();
    popups.dispose();
    loadError.dispose();
    await _navController.close();
    await _pageController.close();
    await _cursorController.close();
  }
}
