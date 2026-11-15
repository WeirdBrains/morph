import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Whether Morph is the system's default web browser.
enum MorphDefaultBrowserState { isDefault, notDefault, unknown }

/// What asking to become the default did.
enum MorphDefaultBrowserRequest {
  /// Morph is the default now.
  madeDefault,

  /// The system's own default-apps screen opened; the user picks there.
  openedSettings,

  /// The user said no in the system prompt.
  declined,

  /// Nothing could be changed on this system.
  failed,
}

/// Reads and sets the default browser. Android asks the platform
/// (RoleManager) over [channel]; other platforms are unsupported here (the
/// desktop shell is the GPUI app).
abstract class MorphDefaultBrowser {
  const MorphDefaultBrowser();

  static const channel = MethodChannel('org.weirdbrains.morph/default_browser');

  factory MorphDefaultBrowser.forPlatform() {
    if (kIsWeb) {
      return const _Unsupported();
    }
    switch (defaultTargetPlatform) {
      case TargetPlatform.android:
        return const MorphChannelDefaultBrowser();
      default:
        return const _Unsupported();
    }
  }

  bool get supported => true;

  Future<MorphDefaultBrowserState> status();

  Future<MorphDefaultBrowserRequest> makeDefault();
}

class _Unsupported extends MorphDefaultBrowser {
  const _Unsupported();

  @override
  bool get supported => false;

  @override
  Future<MorphDefaultBrowserState> status() async =>
      MorphDefaultBrowserState.unknown;

  @override
  Future<MorphDefaultBrowserRequest> makeDefault() async =>
      MorphDefaultBrowserRequest.failed;
}

/// Android (RoleManager.ROLE_BROWSER, or the default-apps screen),
/// implemented natively.
class MorphChannelDefaultBrowser extends MorphDefaultBrowser {
  const MorphChannelDefaultBrowser();

  @override
  Future<MorphDefaultBrowserState> status() async {
    try {
      final value = await MorphDefaultBrowser.channel.invokeMethod<bool>(
        'status',
      );
      if (value == null) {
        return MorphDefaultBrowserState.unknown;
      }
      return value
          ? MorphDefaultBrowserState.isDefault
          : MorphDefaultBrowserState.notDefault;
    } on PlatformException {
      return MorphDefaultBrowserState.unknown;
    } on MissingPluginException {
      return MorphDefaultBrowserState.unknown;
    }
  }

  @override
  Future<MorphDefaultBrowserRequest> makeDefault() async {
    try {
      final value = await MorphDefaultBrowser.channel.invokeMethod<String>(
        'request',
      );
      return MorphDefaultBrowserRequest.values.firstWhere(
        (r) => r.name == value,
        orElse: () => MorphDefaultBrowserRequest.failed,
      );
    } on PlatformException {
      return MorphDefaultBrowserRequest.failed;
    } on MissingPluginException {
      return MorphDefaultBrowserRequest.failed;
    }
  }
}
