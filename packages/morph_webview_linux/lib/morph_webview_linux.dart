import 'dart:async';

import 'package:flutter/services.dart';
import 'package:morph_webview/morph_webview.dart';

/// Linux method-channel implementation of [MorphWebViewPlatform].
class MorphWebViewLinux extends MorphWebViewPlatform {
  MorphWebViewLinux({MethodChannel? channel, EventChannel? events})
    : _channel =
          channel ?? const MethodChannel('org.weirdbrains.morph_webview'),
      _events =
          events ?? const EventChannel('org.weirdbrains.morph_webview/events');

  final MethodChannel _channel;
  final EventChannel _events;
  Stream<Map<dynamic, dynamic>>? _broadcast;

  static void registerWith() {
    MorphWebViewPlatform.instance = MorphWebViewLinux();
  }

  Stream<Map<dynamic, dynamic>> get _eventStream {
    return _broadcast ??= _events.receiveBroadcastStream().map(
      (event) => Map<dynamic, dynamic>.from(event as Map),
    );
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
    final path = MorphRenderPathInfo(
      path: result['renderPath']! as String,
      reason: (result['renderReason'] as String?) ?? '',
    );
    return _LinuxWebViewSession(
      channel: _channel,
      viewId: viewId,
      textureId: textureId,
      renderPath: path,
      events: _eventStream.where((event) => event['viewId'] == viewId),
    );
  }
}

class _LinuxWebViewSession extends MorphWebViewSession {
  _LinuxWebViewSession({
    required MethodChannel channel,
    required this.viewId,
    required this.textureId,
    required this.renderPath,
    required Stream<Map<dynamic, dynamic>> events,
  }) : _channel = channel {
    _sub = events.listen(_onEvent);
  }

  final MethodChannel _channel;
  final int viewId;
  late final StreamSubscription<Map<dynamic, dynamic>> _sub;
  final _navController = StreamController<MorphNavigationEvent>.broadcast();
  final _pageController = StreamController<String>.broadcast();

  @override
  final int textureId;

  @override
  final MorphRenderPathInfo renderPath;

  @override
  Stream<MorphNavigationEvent> get navigation => _navController.stream;

  @override
  Stream<String> get pageMessages => _pageController.stream;

  void _onEvent(Map<dynamic, dynamic> event) {
    final type = event['type'] as String?;
    if (type == 'navigation') {
      _navController.add(
        MorphNavigationEvent(
          url: (event['url'] as String?) ?? '',
          title: (event['title'] as String?) ?? '',
          canGoBack: event['canGoBack'] == true,
          canGoForward: event['canGoForward'] == true,
          isLoading: event['isLoading'] == true,
          progress: (event['progress'] as num?)?.toDouble() ?? 0,
        ),
      );
    } else if (type == 'pageMessage') {
      _pageController.add((event['json'] as String?) ?? '');
    }
  }

  Future<void> _invoke(String method, [Map<String, Object?>? args]) {
    return _channel.invokeMethod<void>(method, {'viewId': viewId, ...?args});
  }

  @override
  Future<void> resize({
    required int width,
    required int height,
    required double devicePixelRatio,
  }) {
    return _invoke('resize', {
      'width': width,
      'height': height,
      'devicePixelRatio': devicePixelRatio,
    });
  }

  @override
  Future<void> loadUrl(String url) => _invoke('loadUrl', {'url': url});

  @override
  Future<void> goBack() => _invoke('goBack');

  @override
  Future<void> goForward() => _invoke('goForward');

  @override
  Future<void> reload() => _invoke('reload');

  @override
  Future<void> sendPointer(MorphPointerEvent event) =>
      _invoke('sendPointer', event.toMap());

  @override
  Future<void> sendKey(MorphKeyEvent event) =>
      _invoke('sendKey', event.toMap());

  @override
  Future<String?> evaluateJavaScript(String script) {
    return _channel.invokeMethod<String>('evaluateJavaScript', {
      'viewId': viewId,
      'script': script,
    });
  }

  @override
  Future<void> dispose() async {
    await _sub.cancel();
    await _invoke('dispose');
    await _navController.close();
    await _pageController.close();
  }
}
