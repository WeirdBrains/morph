import 'dart:io';

import 'package:flutter/foundation.dart';

/// Which chrome buttons Morph should draw itself.
///
/// On macOS the native traffic lights stay visible under a hidden title bar,
/// so the shell does not draw custom buttons (see
/// [MorphWindowChrome.leadingInset]). Windows and Linux draw custom buttons
/// on the right: minimize, maximize, close — unless a tiling compositor is
/// detected, in which case only close is shown.
///
/// Override with `MORPH_WINDOW_BUTTONS=close` or `MORPH_WINDOW_BUTTONS=all`.
enum MorphWindowButtonMode { closeOnly, all }

/// Desktop window-chrome policy: button mode, placement, and macOS padding.
class MorphWindowChrome {
  MorphWindowChrome._();

  /// Environment override: `close` or `all`. Any other value is ignored.
  static const overrideEnvName = 'MORPH_WINDOW_BUTTONS';

  /// Fallback leading inset (logical pixels) reserved for native macOS
  /// traffic lights when their bounds cannot be measured.
  static const macTrafficLightFallbackInset = 78.0;

  /// Gap after the right edge of the measured zoom button.
  static const macTrafficLightGap = 8.0;

  /// Known tiling compositors / window managers (matched case-insensitively
  /// against `XDG_CURRENT_DESKTOP`, `XDG_SESSION_DESKTOP`, and
  /// `DESKTOP_SESSION`).
  static const tilingDesktopIds = {
    'hyprland',
    'hypr',
    'sway',
    'i3',
    'i3-gaps',
    'niri',
    'river',
    'awesome',
    'bspwm',
    'dwm',
    'dwl',
    'qtile',
    'xmonad',
    'herbstluftwm',
    'herbstluft',
    'leftwm',
    'spectrwm',
    'wmii',
    'notion',
    'hikari',
    'newm',
    'stumpwm',
    'subtle',
    'dk',
  };

  /// Environment markers that mean a tiling compositor is running, even if
  /// the XDG desktop fields are empty or stale.
  static const tilingEnvMarkers = {
    'HYPRLAND_INSTANCE_SIGNATURE',
    'SWAYSOCK',
    'I3SOCK',
    'NIRI_SOCKET',
    'BSPWM_SOCKET',
  };

  /// Resolve which custom buttons to show.
  ///
  /// [environment] and [platform] are injectable for tests. When omitted,
  /// the process environment and [defaultTargetPlatform] are used.
  static MorphWindowButtonMode mode({
    TargetPlatform? platform,
    Map<String, String>? environment,
  }) {
    final env = environment ?? _processEnvironment();
    final override = env[overrideEnvName]?.trim().toLowerCase();
    if (override == 'close') {
      return MorphWindowButtonMode.closeOnly;
    }
    if (override == 'all') {
      return MorphWindowButtonMode.all;
    }

    final target = platform ?? defaultTargetPlatform;
    if (target != TargetPlatform.linux) {
      return MorphWindowButtonMode.all;
    }

    for (final marker in tilingEnvMarkers) {
      final value = env[marker];
      if (value != null && value.isNotEmpty) {
        return MorphWindowButtonMode.closeOnly;
      }
    }

    for (final token in desktopTokens(env)) {
      if (tilingDesktopIds.contains(token)) {
        return MorphWindowButtonMode.closeOnly;
      }
    }
    return MorphWindowButtonMode.all;
  }

  /// Tokens from XDG / session desktop variables, lowercased, with path
  /// basenames and a trailing `.desktop` stripped.
  static Iterable<String> desktopTokens(Map<String, String> env) sync* {
    const keys = [
      'XDG_CURRENT_DESKTOP',
      'XDG_SESSION_DESKTOP',
      'DESKTOP_SESSION',
    ];
    for (final key in keys) {
      final raw = env[key];
      if (raw == null || raw.isEmpty) {
        continue;
      }
      for (final part in raw.split(RegExp(r'[:; ]+'))) {
        var token = part.trim().toLowerCase();
        if (token.isEmpty) {
          continue;
        }
        if (token.contains('/')) {
          token = token.split('/').last;
        }
        if (token.endsWith('.desktop')) {
          token = token.substring(0, token.length - '.desktop'.length);
        }
        if (token.isNotEmpty) {
          yield token;
        }
      }
    }
  }

  /// Whether Morph should paint its own window buttons (vs native macOS
  /// traffic lights, or no buttons on mobile).
  static bool showsCustomButtons({
    TargetPlatform? platform,
    bool useNativeMacButtons = true,
  }) {
    final target = platform ?? defaultTargetPlatform;
    switch (target) {
      case TargetPlatform.android:
      case TargetPlatform.iOS:
      case TargetPlatform.fuchsia:
        return false;
      case TargetPlatform.macOS:
        return !useNativeMacButtons;
      case TargetPlatform.linux:
      case TargetPlatform.windows:
        return true;
    }
  }

  /// macOS places chrome on the leading edge; Windows and Linux on the
  /// trailing edge.
  static bool buttonsOnLeadingSide({TargetPlatform? platform}) {
    return (platform ?? defaultTargetPlatform) == TargetPlatform.macOS;
  }

  /// Leading padding so the Morph mark and tabs start after native macOS
  /// traffic lights. Zero on Windows/Linux, and zero when drawing fallback
  /// custom buttons on macOS instead.
  static double leadingInset({
    TargetPlatform? platform,
    bool useNativeMacButtons = true,
    double? measuredTrafficLightRight,
  }) {
    final target = platform ?? defaultTargetPlatform;
    if (target != TargetPlatform.macOS || !useNativeMacButtons) {
      return 0;
    }
    final measured = measuredTrafficLightRight;
    if (measured != null && measured > 40 && measured < 160) {
      return measured + macTrafficLightGap;
    }
    return macTrafficLightFallbackInset;
  }

  static Map<String, String> _processEnvironment() {
    if (kIsWeb) {
      return const {};
    }
    return Platform.environment;
  }
}
