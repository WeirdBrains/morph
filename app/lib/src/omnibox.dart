import 'incoming_links.dart';

/// What typing [input] into the address bar opens: a web address when the
/// text looks like one, else a search with Morph's default engine
/// (DuckDuckGo). Null for blank input.
String? omniboxTarget(String input) {
  final text = input.trim();
  if (text.isEmpty) {
    return null;
  }
  final scheme = _scheme.firstMatch(text)?.group(1)?.toLowerCase();
  if (scheme != null) {
    if (scheme == 'http' || scheme == 'https') {
      final uri = Uri.tryParse(text);
      if (uri != null && uri.host.isNotEmpty && !_whitespace.hasMatch(text)) {
        return text;
      }
      return MorphIncomingLinks.searchUrl(text);
    }
    if (_directSchemes.contains(scheme)) {
      return text;
    }
  }
  // A phrase is a search, even if one of its words looks like a host.
  if (_whitespace.hasMatch(text)) {
    return MorphIncomingLinks.searchUrl(text);
  }
  if (_localHost.hasMatch(text) || _ipv4.hasMatch(text)) {
    return 'http://$text';
  }
  if (_bareHost.hasMatch(text) && Uri.tryParse('https://$text') != null) {
    return 'https://$text';
  }
  return MorphIncomingLinks.searchUrl(text);
}

// `scheme:` followed by `//` or, for the schemes below, anything. A bare
// `host:port` such as `example.com:8080` is not a scheme.
final _scheme = RegExp(
  r'^([a-z][a-z0-9+.\-]*):(?://|(?![0-9]))',
  caseSensitive: false,
);
const _directSchemes = {'about', 'file', 'data', 'view-source'};
final _whitespace = RegExp(r'\s');
final _localHost = RegExp(r'^localhost(:\d+)?(/\S*)?$', caseSensitive: false);
final _ipv4 = RegExp(r'^\d{1,3}(\.\d{1,3}){3}(:\d+)?(/\S*)?$');
final _bareHost = RegExp(
  r'^[a-z0-9-]+(\.[a-z0-9-]+)*\.[a-z]{2,63}\.?(:\d+)?([/?#]\S*)?$',
  caseSensitive: false,
);
