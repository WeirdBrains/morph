import 'package:flutter/material.dart';

import '../theme/morph_theme.dart';

/// Square glitch mark. [semanticLabel] defaults to "Morph".
class MorphMark extends StatelessWidget {
  const MorphMark({super.key, this.size = 28});

  final double size;

  @override
  Widget build(BuildContext context) {
    return Image.asset(
      'assets/brand/morph-mark.png',
      key: const Key('morph-mark'),
      width: size,
      height: size,
      filterQuality: FilterQuality.high,
      semanticLabel: 'Morph',
    );
  }
}

/// Horizontal lockup: rounded icon + geometric mono "morph".
class MorphWordmark extends StatelessWidget {
  const MorphWordmark({super.key, this.height = 72});

  final double height;

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    return Image.asset(
      dark
          ? 'assets/brand/morph-wordmark.png'
          : 'assets/brand/morph-wordmark-light.png',
      key: const Key('morph-wordmark'),
      height: height,
      filterQuality: FilterQuality.high,
      semanticLabel: 'Morph',
    );
  }
}

/// A thin cyan / magenta scanline used under the toolbar.
class MorphGlitchRule extends StatelessWidget {
  const MorphGlitchRule({super.key});

  @override
  Widget build(BuildContext context) {
    return const SizedBox(
      height: 3,
      width: double.infinity,
      child: CustomPaint(painter: _GlitchRulePainter()),
    );
  }
}

class _GlitchRulePainter extends CustomPainter {
  const _GlitchRulePainter();

  @override
  void paint(Canvas canvas, Size size) {
    final y = size.height / 2;
    final base = Paint()
      ..color = MorphColors.borderDark.withValues(alpha: 0.6)
      ..strokeWidth = 1;
    canvas.drawLine(Offset(0, y), Offset(size.width, y), base);

    final cyan = Paint()
      ..color = MorphColors.cyan
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.square;
    final magenta = Paint()
      ..color = MorphColors.magenta
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.square;

    final mid = size.width * 0.22;
    canvas.drawLine(Offset(mid, 0), Offset(mid + 28, 0), cyan);
    canvas.drawLine(
      Offset(mid + 18, size.height),
      Offset(mid + 52, size.height),
      magenta,
    );
    final mid2 = size.width * 0.72;
    canvas.drawLine(Offset(mid2, 0), Offset(mid2 + 36, 0), magenta);
    canvas.drawLine(
      Offset(mid2 - 16, size.height),
      Offset(mid2 + 12, size.height),
      cyan,
    );
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}
