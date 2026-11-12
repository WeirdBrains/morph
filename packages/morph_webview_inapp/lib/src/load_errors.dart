import 'package:flutter_inappwebview/flutter_inappwebview.dart';
import 'package:morph_webview/morph_webview.dart';

/// Which Morph error page a failed main-frame load shows, or null when the
/// failure is not the user's problem (a cancelled or superseded navigation,
/// a scheme handed to another app). Same kinds as the Linux backend, so
/// every platform shows the same pages.
MorphLoadErrorKind? morphInAppLoadErrorKind(WebResourceErrorType type) {
  if (_ignored.contains(type)) {
    return null;
  }
  if (_certificate.contains(type)) {
    return MorphLoadErrorKind.certificate;
  }
  if (type == WebResourceErrorType.HOST_LOOKUP) {
    return MorphLoadErrorKind.notFound;
  }
  if (type == WebResourceErrorType.TIMEOUT) {
    return MorphLoadErrorKind.timedOut;
  }
  if (_unreachable.contains(type)) {
    return MorphLoadErrorKind.unreachable;
  }
  return MorphLoadErrorKind.other;
}

/// The error for a load that failed, or null when no page should show:
/// subresource failures leave the page as it is.
MorphLoadError? morphInAppLoadError({
  required String url,
  required bool isForMainFrame,
  required WebResourceErrorType type,
  String description = '',
}) {
  if (!isForMainFrame) {
    return null;
  }
  final kind = morphInAppLoadErrorKind(type);
  if (kind == null) {
    return null;
  }
  return MorphLoadError(url: url, kind: kind, message: description);
}

/// The URL whose certificate failed when Android reports a bad certificate
/// for [host], or null when the bad certificate belongs to some other host
/// (a subresource; the page itself is fine). [candidates] are the addresses
/// the tab is loading or showing, newest first.
String? morphCertificateErrorUrl(String host, List<String?> candidates) {
  final wanted = host.toLowerCase();
  for (final candidate in candidates) {
    final uri = Uri.tryParse(candidate ?? '');
    if (uri != null && uri.host.toLowerCase() == wanted && wanted.isNotEmpty) {
      return candidate;
    }
  }
  return null;
}

final _ignored = {
  WebResourceErrorType.CANCELLED,
  WebResourceErrorType.USER_CANCELLED_AUTHENTICATION,
  WebResourceErrorType.UNSUPPORTED_SCHEME,
};

final _certificate = {
  WebResourceErrorType.FAILED_SSL_HANDSHAKE,
  WebResourceErrorType.SECURE_CONNECTION_FAILED,
  WebResourceErrorType.SERVER_CERTIFICATE_HAS_BAD_DATE,
  WebResourceErrorType.SERVER_CERTIFICATE_HAS_UNKNOWN_ROOT,
  WebResourceErrorType.SERVER_CERTIFICATE_NOT_YET_VALID,
  WebResourceErrorType.SERVER_CERTIFICATE_UNTRUSTED,
  WebResourceErrorType.CLIENT_CERTIFICATE_REJECTED,
  WebResourceErrorType.CLIENT_CERTIFICATE_REQUIRED,
};

final _unreachable = {
  WebResourceErrorType.CANNOT_CONNECT_TO_HOST,
  WebResourceErrorType.NOT_CONNECTED_TO_INTERNET,
  WebResourceErrorType.NETWORK_CONNECTION_LOST,
  WebResourceErrorType.SERVER_UNREACHABLE,
  WebResourceErrorType.CONNECTION_ABORTED,
  WebResourceErrorType.CANNOT_LOAD_FROM_NETWORK,
  WebResourceErrorType.INTERNATIONAL_ROAMING_OFF,
  WebResourceErrorType.DATA_NOT_ALLOWED,
  WebResourceErrorType.RESET,
};
