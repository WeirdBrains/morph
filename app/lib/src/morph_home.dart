import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

/// On Android and iOS, `dirs::home_dir()` is empty, so Rust cannot fall
/// back to `~/.morph`. Use the app support directory instead.
///
/// Desktop keeps the existing behavior: an explicit `home`, else
/// `MORPH_HOME`, else `~/.morph` inside morph-identity.
Future<String?> defaultMorphHome({
  TargetPlatform? platform,
  Future<Directory> Function()? applicationSupportDirectory,
}) async {
  if (kIsWeb) {
    return null;
  }
  final target = platform ?? defaultTargetPlatform;
  switch (target) {
    case TargetPlatform.android:
    case TargetPlatform.iOS:
      final dir =
          await (applicationSupportDirectory ??
              getApplicationSupportDirectory)();
      return dir.path;
    default:
      return null;
  }
}
