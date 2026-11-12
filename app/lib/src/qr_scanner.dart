import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// The camera QR scanner (Android: CameraX and ZXing in `QrScanActivity`).
/// Desktops have no scanner; they paste the link text instead.
class MorphQrScanner {
  static const channel = MethodChannel('org.weirdbrains.morph/scan');

  static bool get available => !kIsWeb && Platform.isAndroid;

  /// The text of the first QR code the camera sees, or null when the user
  /// backed out, denied the camera, or there is no scanner.
  static Future<String?> scan() async {
    try {
      return await channel.invokeMethod<String>('scan');
    } on MissingPluginException {
      return null;
    } on PlatformException {
      return null;
    }
  }
}
