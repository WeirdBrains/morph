import 'dart:async';
import 'dart:io';

import 'package:fixnum/fixnum.dart';
import 'package:grpc/grpc.dart';
import 'package:morph_webview/morph_webview.dart';

import 'generated/morph.webview.v1.pbgrpc.dart';

/// One in-process gRPC session shared by every Linux view in this isolate.
class MorphControlClient {
  MorphControlClient(this.uri);

  final String uri;

  ClientChannel? _channel;
  WebViewControlClient? _stub;
  final _outbound = StreamController<ClientMessage>();
  StreamSubscription<ServerMessage>? _inbound;
  Int64 _seq = Int64.ZERO;
  Int64 _jsRequest = Int64.ZERO;
  final _nav = StreamController<(int, MorphNavigationEvent)>.broadcast();
  final _page = StreamController<(int, String)>.broadcast();
  final _pendingJs = <int, Completer<String>>{};

  Stream<(int, MorphNavigationEvent)> get navigation => _nav.stream;
  Stream<(int, String)> get pageMessages => _page.stream;

  Future<void> connect() async {
    if (_stub != null) {
      return;
    }
    final channel = _channelFor(uri);
    _channel = channel;
    final stub = WebViewControlClient(channel);
    _stub = stub;
    _inbound = stub
        .session(_outbound.stream)
        .listen(
          _onServer,
          onError: (Object error, StackTrace stack) {
            for (final pending in _pendingJs.values) {
              if (!pending.isCompleted) {
                pending.completeError(error, stack);
              }
            }
            _pendingJs.clear();
          },
        );
  }

  static ClientChannel _channelFor(String uri) {
    if (uri.startsWith('unix://')) {
      final path = uri.substring('unix://'.length);
      return ClientChannel(
        InternetAddress(path, type: InternetAddressType.unix),
        port: 0,
        options: const ChannelOptions(
          credentials: ChannelCredentials.insecure(),
        ),
      );
    }
    final parsed = Uri.parse(
      uri.startsWith('tcp://') ? uri.replaceFirst('tcp://', 'http://') : uri,
    );
    return ClientChannel(
      parsed.host,
      port: parsed.hasPort ? parsed.port : 0,
      options: const ChannelOptions(credentials: ChannelCredentials.insecure()),
    );
  }

  Int64 _nextSeq() {
    _seq += Int64.ONE;
    return _seq;
  }

  void send({
    required int viewId,
    Navigate? navigate,
    GoBack? goBack,
    GoForward? goForward,
    Reload? reload,
    PointerEvent? pointer,
    KeyEvent? key,
    ImeEvent? ime,
    EvaluateJavaScript? evaluateJavascript,
  }) {
    _outbound.add(
      ClientMessage(
        seq: _nextSeq(),
        viewId: Int64(viewId),
        navigate: navigate,
        goBack: goBack,
        goForward: goForward,
        reload: reload,
        pointer: pointer,
        key: key,
        ime: ime,
        evaluateJavascript: evaluateJavascript,
      ),
    );
  }

  Future<String> evaluateJavaScript(int viewId, String script) async {
    _jsRequest += Int64.ONE;
    final requestId = _jsRequest;
    final completer = Completer<String>();
    _pendingJs[requestId.toInt()] = completer;
    send(
      viewId: viewId,
      evaluateJavascript: EvaluateJavaScript(
        requestId: requestId,
        script: script,
      ),
    );
    return completer.future.timeout(
      const Duration(seconds: 8),
      onTimeout: () {
        _pendingJs.remove(requestId.toInt());
        throw TimeoutException('evaluateJavaScript timed out');
      },
    );
  }

  void _onServer(ServerMessage message) {
    final viewId = message.viewId.toInt();
    switch (message.whichBody()) {
      case ServerMessage_Body.navigation:
        final event = message.navigation;
        _nav.add((
          viewId,
          MorphNavigationEvent(
            url: event.url,
            title: event.title,
            canGoBack: event.canGoBack,
            canGoForward: event.canGoForward,
            isLoading: event.isLoading,
            progress: event.progress,
          ),
        ));
      case ServerMessage_Body.pageMessage:
        _page.add((viewId, message.pageMessage.json));
      case ServerMessage_Body.evaluateJavascriptResult:
        final result = message.evaluateJavascriptResult;
        final pending = _pendingJs.remove(result.requestId.toInt());
        if (pending != null && !pending.isCompleted) {
          if (result.error.isNotEmpty) {
            pending.completeError(StateError(result.error));
          } else {
            pending.complete(result.result);
          }
        }
      case ServerMessage_Body.ack:
      case ServerMessage_Body.error:
      case ServerMessage_Body.notSet:
        break;
    }
  }

  Future<void> shutdown() async {
    await _inbound?.cancel();
    _inbound = null;
    await _outbound.close();
    await _channel?.shutdown();
    _channel = null;
    _stub = null;
  }
}
