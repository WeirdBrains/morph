import 'package:flutter/material.dart';

/// Morph brand and theme colours. Dark is the default theme.
///
/// Neon [cyan] and [magenta] are brand fills. They meet WCAG AA on every
/// dark surface; on light surfaces use [cyanText] and [magentaText] for
/// labels and links. See `docs/brand/palette.md` for contrast ratios.
abstract final class MorphColors {
  static const Color charcoal = Color(0xFF0D0D12);
  static const Color white = Color(0xFFFFFFFF);
  static const Color cyan = Color(0xFF00E5FF);
  static const Color magenta = Color(0xFFFF2BD6);

  static const Color backgroundDark = Color(0xFF0D0D12);
  static const Color surfaceDark = Color(0xFF16161D);
  static const Color surfaceVariantDark = Color(0xFF1E1E28);
  static const Color textPrimaryDark = Color(0xFFF5F6FA);
  static const Color textSecondaryDark = Color(0xFFA8AAB8);
  static const Color successDark = Color(0xFF3DFF9A);
  static const Color warningDark = Color(0xFFFFC14D);
  static const Color errorDark = Color(0xFFFF4D6A);
  static const Color infoDark = Color(0xFF6B9BFF);
  static const Color borderDark = Color(0xFF707284);
  static const Color selectionDark = Color(0xFF0A434D);

  static const Color backgroundLight = Color(0xFFF4F5F8);
  static const Color surfaceLight = Color(0xFFFFFFFF);
  static const Color surfaceVariantLight = Color(0xFFE8E9F0);
  static const Color textPrimaryLight = Color(0xFF0D0D12);
  static const Color textSecondaryLight = Color(0xFF4A4C5A);
  static const Color cyanText = Color(0xFF006E8A);
  static const Color magentaText = Color(0xFFB0008A);
  static const Color successLight = Color(0xFF00743F);
  static const Color warningLight = Color(0xFF8A5A00);
  static const Color errorLight = Color(0xFFC4002A);
  static const Color infoLight = Color(0xFF1A4EC8);
  static const Color borderLight = Color(0xFF7A7C8A);
  static const Color selectionLight = Color(0xFFBFF8FF);
}

/// Material 3 [ThemeData] for Morph. Dark is the product default.
abstract final class MorphTheme {
  static const String mono = 'monospace';

  static const ColorScheme darkScheme = ColorScheme(
    brightness: Brightness.dark,
    primary: MorphColors.cyan,
    onPrimary: MorphColors.charcoal,
    primaryContainer: Color(0xFF004E5A),
    onPrimaryContainer: MorphColors.cyan,
    secondary: MorphColors.magenta,
    onSecondary: MorphColors.charcoal,
    secondaryContainer: Color(0xFF5A1048),
    onSecondaryContainer: MorphColors.magenta,
    tertiary: MorphColors.infoDark,
    onTertiary: MorphColors.charcoal,
    error: MorphColors.errorDark,
    onError: MorphColors.charcoal,
    surface: MorphColors.surfaceDark,
    onSurface: MorphColors.textPrimaryDark,
    onSurfaceVariant: MorphColors.textSecondaryDark,
    outline: MorphColors.borderDark,
    outlineVariant: Color(0xFF3A3A4A),
    inverseSurface: MorphColors.surfaceLight,
    onInverseSurface: MorphColors.textPrimaryLight,
    inversePrimary: MorphColors.cyanText,
    surfaceContainerHighest: MorphColors.surfaceVariantDark,
    scrim: Color(0xFF000000),
    shadow: Color(0xFF000000),
  );

  static const ColorScheme lightScheme = ColorScheme(
    brightness: Brightness.light,
    primary: MorphColors.cyanText,
    onPrimary: MorphColors.white,
    primaryContainer: Color(0xFFBFF8FF),
    onPrimaryContainer: MorphColors.cyanText,
    secondary: MorphColors.magentaText,
    onSecondary: MorphColors.white,
    secondaryContainer: Color(0xFFFFD6F1),
    onSecondaryContainer: MorphColors.magentaText,
    tertiary: MorphColors.infoLight,
    onTertiary: MorphColors.white,
    error: MorphColors.errorLight,
    onError: MorphColors.white,
    surface: MorphColors.surfaceLight,
    onSurface: MorphColors.textPrimaryLight,
    onSurfaceVariant: MorphColors.textSecondaryLight,
    outline: MorphColors.borderLight,
    outlineVariant: Color(0xFFC5C7D4),
    inverseSurface: MorphColors.surfaceDark,
    onInverseSurface: MorphColors.textPrimaryDark,
    inversePrimary: MorphColors.cyan,
    surfaceContainerHighest: MorphColors.surfaceVariantLight,
    scrim: Color(0xFF000000),
    shadow: Color(0xFF000000),
  );

  static ThemeData get dark => _theme(
    scheme: darkScheme,
    background: MorphColors.backgroundDark,
    selection: MorphColors.selectionDark,
    cursor: MorphColors.cyan,
    border: MorphColors.borderDark,
  );

  static ThemeData get light => _theme(
    scheme: lightScheme,
    background: MorphColors.backgroundLight,
    selection: MorphColors.selectionLight,
    cursor: MorphColors.cyanText,
    border: MorphColors.borderLight,
  );

  static TextStyle monoStyle({
    required Color color,
    double size = 13,
    FontWeight weight = FontWeight.w400,
  }) {
    return TextStyle(
      fontFamily: mono,
      fontSize: size,
      fontWeight: weight,
      color: color,
      height: 1.3,
    );
  }

  static ThemeData _theme({
    required ColorScheme scheme,
    required Color background,
    required Color selection,
    required Color cursor,
    required Color border,
  }) {
    final radius = BorderRadius.circular(8);
    final enabledBorder = OutlineInputBorder(
      borderRadius: radius,
      borderSide: BorderSide(color: border),
    );
    final focusedBorder = OutlineInputBorder(
      borderRadius: radius,
      borderSide: BorderSide(color: cursor, width: 2),
    );

    return ThemeData(
      useMaterial3: true,
      brightness: scheme.brightness,
      colorScheme: scheme,
      scaffoldBackgroundColor: background,
      canvasColor: scheme.surface,
      dividerColor: border,
      appBarTheme: AppBarTheme(
        backgroundColor: background,
        foregroundColor: scheme.onSurface,
        elevation: 0,
        surfaceTintColor: Colors.transparent,
      ),
      textSelectionTheme: TextSelectionThemeData(
        selectionColor: selection,
        cursorColor: cursor,
        selectionHandleColor: cursor,
      ),
      inputDecorationTheme: InputDecorationTheme(
        isDense: true,
        filled: true,
        fillColor: scheme.surfaceContainerHighest,
        hintStyle: monoStyle(color: scheme.onSurfaceVariant, size: 14),
        labelStyle: TextStyle(color: scheme.onSurfaceVariant),
        border: enabledBorder,
        enabledBorder: enabledBorder,
        focusedBorder: focusedBorder,
        errorBorder: OutlineInputBorder(
          borderRadius: radius,
          borderSide: BorderSide(color: scheme.error),
        ),
        contentPadding: const EdgeInsets.symmetric(
          horizontal: 12,
          vertical: 10,
        ),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          backgroundColor: scheme.primary,
          foregroundColor: scheme.onPrimary,
          shape: RoundedRectangleBorder(borderRadius: radius),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          foregroundColor: scheme.primary,
          side: BorderSide(color: border),
          shape: RoundedRectangleBorder(borderRadius: radius),
        ),
      ),
      iconButtonTheme: IconButtonThemeData(
        style: IconButton.styleFrom(foregroundColor: scheme.onSurface),
      ),
      progressIndicatorTheme: ProgressIndicatorThemeData(color: scheme.primary),
      tooltipTheme: TooltipThemeData(
        decoration: BoxDecoration(
          color: scheme.surfaceContainerHighest,
          borderRadius: radius,
          border: Border.all(color: border),
        ),
        textStyle: TextStyle(color: scheme.onSurface, fontSize: 12),
      ),
      dividerTheme: DividerThemeData(color: border, space: 1, thickness: 1),
      focusColor: cursor.withValues(alpha: 0.24),
      highlightColor: cursor.withValues(alpha: 0.16),
      splashColor: cursor.withValues(alpha: 0.16),
    );
  }
}
