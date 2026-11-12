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

  /// Register the Android WebView that will be built with keep-alive id
  /// [viewKey] for [profileId]'s androidx.webkit profile. Returns whether the
  /// view will be isolated; null on other platforms.
  Future<MorphAndroidProfile?> prepareAndroid(
    String profileId,
    String viewKey,
  ) async {
    if (kIsWeb || !Platform.isAndroid) {
      return null;
    }
    return prepareAndroidVia(macosChannel, profileId, viewKey);
  }

  /// Forget a keep-alive id registered with [prepareAndroid].
  Future<void> forgetAndroidView(String viewKey) async {
    if (kIsWeb || !Platform.isAndroid) {
      return;
    }
    try {
      await macosChannel.invokeMethod<void>('forgetView', {'viewKey': viewKey});
    } on MissingPluginException {
      // Host without the native hook.
    }
  }

  /// Delete [profileId]'s androidx.webkit profile (cookies, storage, cache).
  Future<bool> deleteAndroid(String profileId) async {
    if (kIsWeb || !Platform.isAndroid) {
      return false;
    }
    try {
      return await macosChannel.invokeMethod<bool>('deleteAndroid', {
            'profileId': profileId,
          }) ??
          false;
    } on MissingPluginException {
      return false;
    }
  }

  /// Delete every Morph androidx.webkit profile whose Morph profile is gone.
  Future<void> pruneAndroid(Iterable<String> liveProfileIds) async {
    if (kIsWeb || !Platform.isAndroid) {
      return;
    }
    List<String> names;
    try {
      names =
          await macosChannel.invokeListMethod<String>('listAndroid') ??
          const [];
    } on MissingPluginException {
      return;
    }
    for (final id in staleAndroidProfiles(names, liveProfileIds)) {
      await deleteAndroid(id);
    }
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

/// Result of preparing an Android WebView for a Morph profile.
class MorphAndroidProfile {
  const MorphAndroidProfile({required this.isolated, required this.name});

  /// True when the WebView will use storage no other Morph profile shares.
  final bool isolated;

  /// androidx.webkit profile name (`Default` for Morph's default profile).
  final String name;
}

/// androidx.webkit profile name for a Morph profile id; mirrors
/// `WebViewProfiles.androidProfileName` in the app's MainActivity.
String morphAndroidProfileName(String profileId) =>
    profileId.isEmpty || profileId == 'default'
    ? 'Default'
    : 'morph-$profileId';

/// Inline page shown instead of a site when this device's WebView cannot keep
/// a non-default profile's data apart.
const morphAndroidUnisolatedHtml = '''<!doctype html>
<html><head><meta name="viewport" content="width=device-width,initial-scale=1">
<style>body{background:#0d0d12;color:#f5f6fa;font:16px system-ui,sans-serif;padding:24px}
h1{color:#00e5ff;font-size:20px}code{color:#ff2bd6}</style></head>
<body><h1>This profile can't open pages on this device</h1>
<p>Morph keeps every profile's cookies and site data separate. This phone's
Android System WebView is too old to do that, so Morph won't load pages here
rather than mix this profile's data with your default profile.</p>
<p>Update <code>Android System WebView</code> from the Play Store, then reopen
Morph. Your default profile still works.</p></body></html>''';

final morphProfileStore = MorphProfileStore();

/// Platform-independent half of [MorphProfileStore.prepareAndroid], for tests.
@visibleForTesting
Future<MorphAndroidProfile> prepareAndroidVia(
  MethodChannel channel,
  String profileId,
  String viewKey,
) async {
  try {
    final reply = await channel.invokeMapMethod<String, Object?>(
      'prepareAndroid',
      {'profileId': profileId, 'viewKey': viewKey},
    );
    return MorphAndroidProfile(
      isolated: reply?['isolated'] == true,
      name: reply?['profile'] as String? ?? 'Default',
    );
  } on MissingPluginException {
    // No native hook: only the default profile is safe to show.
    return MorphAndroidProfile(
      isolated: morphAndroidProfileName(profileId) == 'Default',
      name: 'Default',
    );
  }
}

/// Morph profile ids whose androidx.webkit profile in [androidNames] has no
/// live Morph profile in [liveProfileIds]. Non-Morph names are left alone.
List<String> staleAndroidProfiles(
  Iterable<String> androidNames,
  Iterable<String> liveProfileIds,
) {
  final live = liveProfileIds.map(morphAndroidProfileName).toSet();
  return [
    for (final name in androidNames)
      if (name.startsWith('morph-') && !live.contains(name))
        name.substring('morph-'.length),
  ];
}
