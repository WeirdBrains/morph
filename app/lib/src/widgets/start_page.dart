import 'package:flutter/material.dart';

import '../theme/morph_theme.dart';
import 'brand.dart';

/// Empty / new-tab start page: wordmark, a quiet glitch accent, and a hint.
class MorphStartPage extends StatelessWidget {
  const MorphStartPage({super.key});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;

    return DecoratedBox(
      key: const Key('start-page'),
      decoration: BoxDecoration(color: theme.scaffoldBackgroundColor),
      child: Stack(
        fit: StackFit.expand,
        children: [
          const CustomPaint(painter: _StartGlitchPainter()),
          SafeArea(
            child: Center(
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 32),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const MorphWordmark(height: 88),
                    const SizedBox(height: 28),
                    Text(
                      'A distributed web browser with local AI.',
                      textAlign: TextAlign.center,
                      style: MorphTheme.monoStyle(
                        color: colors.onSurfaceVariant,
                        size: 14,
                      ),
                    ),
                    const SizedBox(height: 8),
                    Text(
                      'Type an address.',
                      textAlign: TextAlign.center,
                      style: MorphTheme.monoStyle(
                        color: colors.onSurfaceVariant.withValues(alpha: 0.8),
                        size: 13,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// Loose cyan / magenta slices, kept away from the centred wordmark.
class _StartGlitchPainter extends CustomPainter {
  const _StartGlitchPainter();

  @override
  void paint(Canvas canvas, Size size) {
    final cyan = Paint()..color = MorphColors.cyan.withValues(alpha: 0.55);
    final magenta = Paint()
      ..color = MorphColors.magenta.withValues(alpha: 0.55);

    void bar(Paint paint, double x, double y, double w, double h) {
      canvas.drawRect(Rect.fromLTWH(x, y, w, h), paint);
    }

    final cx = size.width / 2;
    final cy = size.height / 2;
    // Left of the wordmark
    bar(cyan, cx - 420, cy - 36, 72, 8);
    bar(magenta, cx - 390, cy - 20, 40, 5);
    bar(cyan, cx - 430, cy + 18, 56, 8);
    // Right of the wordmark
    bar(magenta, cx + 340, cy - 28, 80, 8);
    bar(cyan, cx + 360, cy - 10, 36, 5);
    bar(magenta, cx + 348, cy + 22, 64, 8);
    // Quiet far-edge ticks
    bar(cyan, 24, cy - 80, 48, 6);
    bar(magenta, size.width - 80, cy + 70, 52, 6);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}
