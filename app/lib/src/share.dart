import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Shares a page link: the system share sheet on Android, otherwise the
/// clipboard. Returns true when the share sheet opened.
abstract final class MorphShare {
  static const channel = MethodChannel('org.weirdbrains.morph/share');

  static Future<bool> shareLink(String url, {String title = ''}) async {
    if (!kIsWeb && defaultTargetPlatform == TargetPlatform.android) {
      try {
        final opened = await channel.invokeMethod<bool>('shareText', {
          'text': url,
          'subject': title,
        });
        if (opened ?? false) {
          return true;
        }
      } on PlatformException {
        // Fall back to the clipboard below.
      } on MissingPluginException {
        // Same.
      }
    }
    await Clipboard.setData(ClipboardData(text: url));
    return false;
  }
}
