import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:morph_webview/control.dart' as proto;
import 'package:morph_webview/morph_webview.dart';

/// Linux implementation of [MorphWebViewPlatform].
///
/// Lifecycle and texture ids stay on the method channel. Navigation, input,
/// and the page bridge go over the in-process gRPC control session.
class MorphWebViewLinux extends MorphWebViewPlatform {
  MorphWebViewLinux({MethodChannel? channel, EventChannel? events})
    : _channel =
          channel ?? const MethodChannel('org.weirdbrains.morph_webview'),
      _events =
          events ?? const EventChannel('org.weirdbrains.morph_webview/events');

  final MethodChannel _channel;
  final EventChannel _events;
  MorphControlClient? _control;
  Stream<Map<Object?, Object?>>? _eventStream;

  /// Native per-view events (renderer faults, new windows), shared by all
  /// sessions; each session keeps the ones for its own view id.
  Stream<Map<Object?, Object?>> get _nativeEvents => _eventStream ??= _events
      .receiveBroadcastStream()
      .where((event) => event is Map)
      .cast<Map<Object?, Object?>>()
      .asBroadcastStream();

  static void registerWith() {
    MorphWebViewPlatform.instance = MorphWebViewLinux();
  }

  /// A session over [events] without a native view, for tests.
  @visibleForTesting
  static MorphWebViewSession sessionForTesting({
    required MethodChannel channel,
    required int viewId,
    required Stream<Map<Object?, Object?>> events,
  }) {
    return _LinuxWebViewSession(
      channel: channel,
      control: MorphControlClient('unix:///dev/null'),
      viewId: viewId,
      textureId: viewId,
      renderPath: const MorphRenderPathInfo(path: 'test', reason: 'test'),
      events: events,
    );
  }

  Future<MorphControlClient> _connect(String endpoint) async {
    final existing = _control;
    if (existing != null && existing.uri == endpoint) {
      return existing;
    }
    final client = MorphControlClient(endpoint);
    await client.connect();
    _control = client;
    return client;
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
    final result = await _channel.invokeMapMethod<String, Object?>('create', {
      'initialUrl': initialUrl,
      'width': width,
      'height': height,
      'devicePixelRatio': devicePixelRatio,
      'userScript': userScript,
      'profileId': profileId,
    });
    if (result == null) {
      throw StateError('morph_webview_linux create returned null');
    }
    final viewId = result['viewId']! as int;
    final textureId = result['textureId']! as int;
    final endpoint = (result['controlEndpoint'] as String?) ?? '';
    if (endpoint.isEmpty) {
      throw StateError('morph_webview_linux did not return a control endpoint');
    }
    final path = MorphRenderPathInfo(
      path: result['renderPath']! as String,
      reason: (result['renderReason'] as String?) ?? '',
    );
    final control = await _connect(endpoint);
    return _LinuxWebViewSession(
      channel: _channel,
      control: control,
      viewId: viewId,
      textureId: textureId,
      renderPath: path,
      events: _nativeEvents,
    );
  }
}

class _LinuxWebViewSession extends MorphWebViewSession {
  _LinuxWebViewSession({
    required this._channel,
    required this._control,
    required this.viewId,
    required this.textureId,
    required this.renderPath,
    required Stream<Map<Object?, Object?>> events,
  }) {
    _eventSub = events.listen((event) {
      if (event['viewId'] != viewId || event['type'] != 'renderer') {
        return;
      }
      _rendererController.add(
        MorphRendererEvent(morphRendererFaultFromName(event['fault'])),
      );
    });
    _navSub = _control.navigation.listen((entry) {
      if (entry.$1 == viewId) {
        _navController.add(entry.$2);
      }
    });
    _pageSub = _control.pageMessages.listen((entry) {
      if (entry.$1 == viewId) {
        _pageController.add(entry.$2);
      }
    });
    _cursorSub = _control.cursor.listen((entry) {
      if (entry.$1 == viewId) {
        _cursorController.add(entry.$2);
      }
    });
  }

  final MethodChannel _channel;
  final MorphControlClient _control;
  final int viewId;
  late final StreamSubscription<(int, MorphNavigationEvent)> _navSub;
  late final StreamSubscription<(int, String)> _pageSub;
  late final StreamSubscription<(int, MorphCursorEvent)> _cursorSub;
  late final StreamSubscription<Map<Object?, Object?>> _eventSub;
  final _rendererController = StreamController<MorphRendererEvent>.broadcast();
  final _navController = StreamController<MorphNavigationEvent>.broadcast();
  final _pageController = StreamController<String>.broadcast();
  final _cursorController = StreamController<MorphCursorEvent>.broadcast();

  @override
  final int textureId;

  @override
  final MorphRenderPathInfo renderPath;

  @override
  bool get handlesPageBridge => true;

  @override
  Stream<MorphNavigationEvent> get navigation => _navController.stream;

  @override
  Stream<String> get pageMessages => _pageController.stream;

  @override
  Stream<MorphCursorEvent> get cursor => _cursorController.stream;

  @override
  Stream<MorphRendererEvent> get renderer => _rendererController.stream;

  @override
  Future<void> terminateRenderer() =>
      _channel.invokeMethod<void>('terminateWebProcess', {'viewId': viewId});

  /// The web view survives its web process; the controller's reload of the
  /// last URL starts a new one, so there is nothing to rebuild.
  @override
  Future<void> recoverRenderer() async {}

  @override
  Future<void> resize({
    required int width,
    required int height,
    required double devicePixelRatio,
  }) {
    return _channel.invokeMethod<void>('resize', {
      'viewId': viewId,
      'width': width,
      'height': height,
      'devicePixelRatio': devicePixelRatio,
    });
  }

  @override
  Future<void> loadUrl(String url) async {
    _control.send(
      viewId: viewId,
      navigate: proto.Navigate(url: url),
    );
  }

  @override
  Future<void> goBack() async {
    _control.send(viewId: viewId, goBack: proto.GoBack());
  }

  @override
  Future<void> goForward() async {
    _control.send(viewId: viewId, goForward: proto.GoForward());
  }

  @override
  Future<void> reload() async {
    _control.send(viewId: viewId, reload: proto.Reload());
  }

  @override
  Future<void> sendPointer(MorphPointerEvent event) async {
    _control.send(
      viewId: viewId,
      pointer: proto.PointerEvent(
        phase: event.phase,
        x: event.x,
        y: event.y,
        button: event.button,
        buttons: event.buttons,
        scrollDeltaX: event.scrollDeltaX,
        scrollDeltaY: event.scrollDeltaY,
        ctrl: event.ctrl,
        alt: event.alt,
        shift: event.shift,
        meta: event.meta,
      ),
    );
  }

  @override
  Future<void> sendKey(MorphKeyEvent event) async {
    _control.send(
      viewId: viewId,
      key: proto.KeyEvent(
        phase: event.phase,
        key: event.key,
        hardwareKeycode: event.hardwareKeycode,
        ctrl: event.ctrl,
        alt: event.alt,
        shift: event.shift,
        meta: event.meta,
        text: event.text,
        repeat: event.repeat,
      ),
    );
  }

  @override
  Future<void> sendIme(MorphImeEvent event) async {
    _control.send(
      viewId: viewId,
      ime: proto.ImeEvent(phase: event.phase, text: event.text),
    );
  }

  @override
  Future<String?> evaluateJavaScript(String script) {
    return _control.evaluateJavaScript(viewId, script);
  }

  @override
  Future<void> dispose() async {
    await _navSub.cancel();
    await _pageSub.cancel();
    await _cursorSub.cancel();
    await _eventSub.cancel();
    await _channel.invokeMethod<void>('dispose', {'viewId': viewId});
    await _navController.close();
    await _pageController.close();
    await _cursorController.close();
    await _rendererController.close();
  }
}

/// Maps a native fault name ("crashed", "outOfMemory", "killed",
/// "unresponsive") to a fault; null means healthy.
MorphRendererFault? morphRendererFaultFromName(Object? name) {
  return switch (name) {
    'crashed' => MorphRendererFault.crashed,
    'outOfMemory' => MorphRendererFault.outOfMemory,
    'killed' => MorphRendererFault.killed,
    'unresponsive' => MorphRendererFault.unresponsive,
    null => null,
    // A reason this build does not know yet still means the page is gone.
    _ => MorphRendererFault.crashed,
  };
}
