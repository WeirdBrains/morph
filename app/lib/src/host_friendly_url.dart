/// Short, host-first form of a URL for the address bar and tab titles.
String hostFriendlyUrl(String url) {
  final uri = Uri.tryParse(url);
  if (uri == null || uri.host.isEmpty) {
    return url;
  }
  var host = uri.host;
  if (host.startsWith('www.')) {
    host = host.substring(4);
  }
  final path = uri.path;
  final hasPath = path.isNotEmpty && path != '/';
  if (!hasPath && !uri.hasQuery && (uri.fragment.isEmpty)) {
    return host;
  }
  final buffer = StringBuffer(host);
  if (hasPath) {
    buffer.write(path);
  }
  if (uri.hasQuery) {
    buffer.write('?');
    buffer.write(uri.query);
  }
  if (uri.fragment.isNotEmpty) {
    buffer.write('#');
    buffer.write(uri.fragment);
  }
  return buffer.toString();
}
