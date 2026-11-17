import 'package:morph/src/incoming_links.dart';

/// [FakeMorphApi]'s address-bar rule: a Dart copy of `morph-core`'s
/// `omnibox::target` (the app calls the Rust one through the bridge), so
/// widget tests run without the native library. `omnibox_test.dart` holds it
/// to the same cases as the Rust tests.
String? omniboxRule(String input, {String? engine}) {
  final text = input.trim();
  if (text.isEmpty) {
    return null;
  }
  final scheme = _scheme.firstMatch(text)?.group(1)?.toLowerCase();
  if (scheme != null && isOwnScheme(scheme)) {
    // morph:// and weirdnet-<app>:// are Morph's to open, never a search.
    return text;
  }
  if (engine != null) {
    final search = _searchFor(engine);
    final target = omniboxRule(text);
    final ddg = MorphIncomingLinks.searchUrl(text);
    return target == ddg ? search(text) : target;
  }
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

/// `morph` or `weirdnet` / `weirdnet-<app>` (`morph_core::address`).
bool isOwnScheme(String scheme) {
  final s = scheme.toLowerCase();
  return s == 'morph' ||
      s == 'weirdnet' ||
      (s.startsWith('weirdnet-') && s.length > 'weirdnet-'.length);
}

String Function(String) _searchFor(String engine) {
  if (engine == 'duckduckgo') {
    return MorphIncomingLinks.searchUrl;
  }
  return (q) {
    final text = q.replaceAll(RegExp(r'\s+'), ' ').trim();
    return Uri.https('www.google.com', '/search', {'q': text}).toString();
  };
}

/// Morph's pages (`morph_core::address::PAGES`).
const morphPages = [
  'about',
  'bookmarks',
  'downloads',
  'history',
  'identity',
  'profiles',
  'relays',
  'search',
  'settings',
];
