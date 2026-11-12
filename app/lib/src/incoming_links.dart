import 'dart:async';

import 'package:flutter/services.dart';

/// Web links handed to Morph by the system: command-line arguments on Linux
/// and Windows (the desktop entry's `%u`, the registry's `"%1"`), and the
/// platform channel on Android (ACTION_VIEW intents) and macOS (Apple
/// Events). The shell opens each one in a new tab of the active profile.
class MorphIncomingLinks {
  MorphIncomingLinks({this.arguments = const [], MethodChannel? channel})
    : _channel = channel ?? defaultChannel;

  static const defaultChannel = MethodChannel('org.weirdbrains.morph/links');

  /// The process arguments Morph was started with.
  final List<String> arguments;
  final MethodChannel _channel;
  final _later = StreamController<String>.broadcast();
  bool _listening = false;

  /// Only http(s) URLs with a host; anything else (flags, files) is ignored.
  static bool isWebLink(String value) {
    final uri = Uri.tryParse(value.trim());
    return uri != null &&
        (uri.scheme == 'http' || uri.scheme == 'https') &&
        uri.host.isNotEmpty;
  }

  /// Links Morph was launched with. Call once.
  Future<List<String>> initialLinks() async {
    final links = [
      for (final arg in arguments)
        if (isWebLink(arg)) arg.trim(),
    ];
    try {
      final launch = await _channel.invokeMethod<String>('initialLink');
      if (launch != null && isWebLink(launch)) {
        links.add(launch);
      }
    } on MissingPluginException {
      // Linux and Windows pass links as arguments only.
    } on PlatformException {
      // No launch link.
    }
    return links;
  }

  /// Links opened while Morph runs.
  Stream<String> get later {
    if (!_listening) {
      _listening = true;
      _channel.setMethodCallHandler((call) async {
        final url = call.arguments;
        if (call.method == 'open' && url is String && isWebLink(url)) {
          _later.add(url);
        }
        return null;
      });
    }
    return _later.stream;
  }

  Future<void> dispose() async {
    if (_listening) {
      _channel.setMethodCallHandler(null);
    }
    await _later.close();
  }
}
