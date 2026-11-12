import 'package:flutter_inappwebview/flutter_inappwebview.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph_webview/morph_webview.dart';
import 'package:morph_webview_inapp/src/load_errors.dart';
import 'package:morph_webview_inapp/src/settings.dart';

void main() {
  test('engine errors map to the same pages as Linux', () {
    final cases = {
      WebResourceErrorType.HOST_LOOKUP: MorphLoadErrorKind.notFound,
      WebResourceErrorType.TIMEOUT: MorphLoadErrorKind.timedOut,
      WebResourceErrorType.FAILED_SSL_HANDSHAKE: MorphLoadErrorKind.certificate,
      WebResourceErrorType.SERVER_CERTIFICATE_UNTRUSTED:
          MorphLoadErrorKind.certificate,
      WebResourceErrorType.CANNOT_CONNECT_TO_HOST:
          MorphLoadErrorKind.unreachable,
      WebResourceErrorType.NOT_CONNECTED_TO_INTERNET:
          MorphLoadErrorKind.unreachable,
      WebResourceErrorType.UNKNOWN: MorphLoadErrorKind.other,
    };
    cases.forEach((type, kind) {
      expect(morphInAppLoadErrorKind(type), kind, reason: '$type');
    });
  });

  test('cancelled loads and handed-off schemes show no page', () {
    expect(morphInAppLoadErrorKind(WebResourceErrorType.CANCELLED), isNull);
    expect(
      morphInAppLoadErrorKind(WebResourceErrorType.UNSUPPORTED_SCHEME),
      isNull,
    );
  });

  test('only the main frame gets an error page', () {
    final sub = morphInAppLoadError(
      url: 'https://cdn.example/x.js',
      isForMainFrame: false,
      type: WebResourceErrorType.HOST_LOOKUP,
    );
    expect(sub, isNull);
    final main = morphInAppLoadError(
      url: 'https://nope.invalid/',
      isForMainFrame: true,
      type: WebResourceErrorType.HOST_LOOKUP,
      description: 'net::ERR_NAME_NOT_RESOLVED',
    )!;
    expect(main.kind, MorphLoadErrorKind.notFound);
    expect(main.url, 'https://nope.invalid/');
    expect(main.message, 'net::ERR_NAME_NOT_RESOLVED');
  });

  test('a bad certificate is the page\'s only when it is the page\'s host', () {
    expect(
      morphCertificateErrorUrl('instgram.com', [
        'https://instgram.com/',
        'https://example.com/',
      ]),
      'https://instgram.com/',
    );
    expect(
      morphCertificateErrorUrl('INSTGRAM.com', [null, 'https://instgram.com/']),
      'https://instgram.com/',
    );
    expect(
      morphCertificateErrorUrl('ads.example', ['https://news.example/']),
      isNull,
    );
    expect(morphCertificateErrorUrl('', ['https://news.example/']), isNull);
  });

  test("Chromium's own error page is off", () {
    expect(morphInAppWebViewSettings().disableDefaultErrorPage, isTrue);
  });
}
