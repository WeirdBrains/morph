import 'dart:async';

import 'package:flutter/services.dart';

/// Web links handed to Morph by the system: command-line arguments on Linux
/// and Windows (the desktop entry's `%u`, the registry's `"%1"`), and the
/// platform channel on Android (ACTION_VIEW intents) and macOS (Apple
/// Events). The shell opens each one in a new tab of the active profile.
///
/// On Android, text shared to Morph (ACTION_SEND / SEND_MULTIPLE) or picked
/// from "Search in Morph" (ACTION_PROCESS_TEXT) arrives too: its first web
/// link is opened, or, with no link in it, the text is searched
/// ([sharedTarget]). Both come out of [initialLinks] and [later] as URLs.
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

  /// Where shared text should go: the first web link in it (a lone
  /// `host.tld/path` counts), else a search for the text.
  static String sharedTarget(String text) {
    final trimmed = text.trim();
    final match = _linkInText.firstMatch(trimmed);
    if (match != null) {
      final link = match.group(0)!.replaceFirst(_trailingPunctuation, '');
      if (isWebLink(link)) {
        return link;
      }
    }
    if (_bareHost.hasMatch(trimmed) && isWebLink('https://$trimmed')) {
      return 'https://$trimmed';
    }
    return searchUrl(trimmed);
  }

  /// Morph's default web search (no search-engine setting yet).
  static String searchUrl(String query) {
    final q = query.replaceAll(RegExp(r'\s+'), ' ').trim();
    final capped = q.length > 2000 ? q.substring(0, 2000) : q;
    return Uri.https('duckduckgo.com', '/', {'q': capped}).toString();
  }

  static final _linkInText = RegExp(
    r'https?://[^\s<>"]+',
    caseSensitive: false,
  );
  static final _trailingPunctuation = RegExp(r"[).,;:!?'\]>]+$");
  static final _bareHost = RegExp(
    r'^[a-z0-9-]+(\.[a-z0-9-]+)*\.[a-z]{2,}(:\d+)?(/\S*)?$',
    caseSensitive: false,
  );

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
      return links;
    } on PlatformException {
      // No launch link.
    }
    try {
      final shared = await _channel.invokeMethod<String>('initialShare');
      if (shared != null && shared.trim().isNotEmpty) {
        links.add(sharedTarget(shared));
      }
    } on MissingPluginException {
      // Only Android shares text.
    } on PlatformException {
      // Nothing shared.
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
        } else if (call.method == 'share' &&
            url is String &&
            url.trim().isNotEmpty) {
          _later.add(sharedTarget(url));
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
