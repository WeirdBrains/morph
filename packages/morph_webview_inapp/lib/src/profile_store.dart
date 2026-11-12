import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_inappwebview/flutter_inappwebview.dart';

/// On-disk location of a profile's platform web-view store, matching
/// morph-core: `<morph home>/profiles/<id>/...`.
String morphProfileStorageDir({
  required String profileId,
  required String leaf,
  String? morphHome,
  String? userHome,
  String? pathSeparator,
}) {
  final sep = pathSeparator ?? Platform.pathSeparator;
  final home = _morphHome(morphHome: morphHome, userHome: userHome, sep: sep);
  return '$home${sep}profiles$sep$profileId$sep$leaf';
}

String _morphHome({
  required String? morphHome,
  required String? userHome,
  required String sep,
}) {
  final env = morphHome ?? Platform.environment['MORPH_HOME'];
  if (env != null && env.isNotEmpty) {
    return env;
  }
  final home =
      userHome ??
      Platform.environment['USERPROFILE'] ??
      Platform.environment['HOME'] ??
      '';
  return '$home$sep.morph';
}

/// Per-profile [WebViewEnvironment] (Windows) and a macOS data-store hook.
class MorphProfileStore {
  MorphProfileStore({
    this.macosChannel = const MethodChannel(
      'org.weirdbrains.morph_webview/profile',
    ),
  });

  @visibleForTesting
  final MethodChannel macosChannel;

  final Map<String, Future<WebViewEnvironment>> _windows = {};

  /// Windows WebView2 user data folder for [profileId], or null on other OSes.
  Future<WebViewEnvironment?> windowsEnvironment(String profileId) async {
    if (kIsWeb || !Platform.isWindows) {
      return null;
    }
    return _windows.putIfAbsent(profileId, () async {
      final dir = morphProfileStorageDir(
        profileId: profileId,
        leaf: 'webview2',
      );
      await Directory(dir).create(recursive: true);
      return WebViewEnvironment.create(
        settings: WebViewEnvironmentSettings(userDataFolder: dir),
      );
    });
  }

  /// Tell the macOS hook which [WKWebsiteDataStore] the next WKWebView should
  /// use. No-op on other platforms or when the native plugin is absent.
  Future<void> prepareMacos(String profileId) async {
    if (kIsWeb || !Platform.isMacOS) {
      return;
    }
    try {
      await macosChannel.invokeMethod<void>('prepare', {
        'profileId': profileId,
      });
    } on MissingPluginException {
      // The Dart-only plugin path has no native hook yet in some hosts.
    } on PlatformException {
      // macOS < 14 cannot create a store for an identifier.
    }
  }
}

final morphProfileStore = MorphProfileStore();
