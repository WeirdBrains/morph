import 'dart:async';

import 'package:flutter/services.dart';
import 'package:morph_webview/morph_webview.dart';

import 'src/control_client.dart';
import 'src/generated/morph.webview.v1.pb.dart' as proto;

/// Linux implementation of [MorphWebViewPlatform].
///
/// Lifecycle and texture ids stay on the method channel. Navigation, input,
/// and the page bridge go over the in-process gRPC control session.
class MorphWebViewLinux extends MorphWebViewPlatform {
  MorphWebViewLinux({MethodChannel? channel})
    : _channel =
          channel ?? const MethodChannel('org.weirdbrains.morph_webview');

  final MethodChannel _channel;
  MorphControlClient? _control;

  static void registerWith() {
    MorphWebViewPlatform.instance = MorphWebViewLinux();
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
  }) async {
    final result = await _channel.invokeMapMethod<String, Object?>('create', {
      'initialUrl': initialUrl,
      'width': width,
      'height': height,
      'devicePixelRatio': devicePixelRatio,
      'userScript': userScript,
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
  }) {
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
  }

  final MethodChannel _channel;
  final MorphControlClient _control;
  final int viewId;
  late final StreamSubscription<(int, MorphNavigationEvent)> _navSub;
  late final StreamSubscription<(int, String)> _pageSub;
  final _navController = StreamController<MorphNavigationEvent>.broadcast();
  final _pageController = StreamController<String>.broadcast();

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
        scrollDeltaX: event.scrollDeltaX,
        scrollDeltaY: event.scrollDeltaY,
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
      ),
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
    await _channel.invokeMethod<void>('dispose', {'viewId': viewId});
    await _navController.close();
    await _pageController.close();
  }
}
