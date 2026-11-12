import 'package:flutter/material.dart';

import '../api.dart';
import '../theme/morph_theme.dart';

/// Glitch palette used for profile avatars.
abstract final class MorphProfileColors {
  static const List<Color> palette = [
    MorphColors.cyan,
    MorphColors.magenta,
    MorphColors.successDark,
    MorphColors.warningDark,
    MorphColors.infoDark,
    MorphColors.errorDark,
    Color(0xFFB388FF),
    Color(0xFF64FFDA),
  ];

  /// Stable color for [profile]: an explicit hex if set, otherwise a hash of
  /// the profile id into [palette].
  static Color of(ProfileInfo profile) {
    final explicit = tryParse(profile.color);
    if (explicit != null) {
      return explicit;
    }
    return forId(profile.id);
  }

  static Color forId(String id) {
    var hash = 0;
    for (final unit in id.codeUnits) {
      hash = (hash * 31 + unit) & 0x7fffffff;
    }
    return palette[hash % palette.length];
  }

  static Color? tryParse(String raw) {
    final value = raw.trim();
    if (value.isEmpty) {
      return null;
    }
    var hex = value.startsWith('#') ? value.substring(1) : value;
    if (hex.length == 6) {
      hex = 'FF$hex';
    }
    if (hex.length != 8) {
      return null;
    }
    final parsed = int.tryParse(hex, radix: 16);
    if (parsed == null) {
      return null;
    }
    return Color(parsed);
  }

  static String token(Color color) {
    final value = color.toARGB32() & 0x00ffffff;
    return '#${value.toRadixString(16).padLeft(6, '0')}';
  }

  /// First letter of the display name, or '?' if empty.
  static String initial(String name) {
    final trimmed = name.trim();
    if (trimmed.isEmpty) {
      return '?';
    }
    return trimmed.substring(0, 1).toUpperCase();
  }
}
