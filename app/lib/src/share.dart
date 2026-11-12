import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Shares a page link: the system share sheet on Android, otherwise the
/// clipboard. Returns true when the share sheet opened.
abstract final class MorphShare {
  static const channel = MethodChannel('org.weirdbrains.morph/share');

  static Future<bool> shareLink(String url, {String title = ''}) async {
    if (await shareText(url, subject: title)) {
      return true;
    }
    await Clipboard.setData(ClipboardData(text: url));
    return false;
  }

  /// Opens the Android share sheet for [text]. False where there is none
  /// (desktop) or it could not open; the caller picks a fallback.
  static Future<bool> shareText(String text, {String subject = ''}) async {
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.android) {
      return false;
    }
    try {
      final opened = await channel.invokeMethod<bool>('shareText', {
        'text': text,
        'subject': subject,
      });
      return opened ?? false;
    } on PlatformException {
      return false;
    } on MissingPluginException {
      return false;
    }
  }
}
