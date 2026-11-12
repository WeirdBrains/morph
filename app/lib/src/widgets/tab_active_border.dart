import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../theme/morph_theme.dart';

/// Which edge of a tab carries the active Glitch border.
enum MorphTabActiveEdge { top, bottom, left, right }

/// Cyan (and a short magenta offset) stroke along a tab's decorated edge.
///
/// Top tabs use the top edge, including the rounded corners. Bottom tabs
/// mirror that on the bottom. Side lists put the mark on the inner edge
/// facing the page: the right side of a left rail, the left side of a
/// right rail.
class MorphTabActiveBorder extends StatelessWidget {
  const MorphTabActiveBorder({
    super.key,
    required this.edge,
    this.cornerRadius = 8,
    this.strokeWidth = defaultStrokeWidth,
  });

  static const double defaultStrokeWidth = 2;
  static const double bandExtent = 10;

  final MorphTabActiveEdge edge;
  final double cornerRadius;
  final double strokeWidth;

  bool get isHorizontal =>
      edge == MorphTabActiveEdge.top || edge == MorphTabActiveEdge.bottom;

  @override
  Widget build(BuildContext context) {
    return CustomPaint(
      painter: _MorphTabActiveBorderPainter(
        edge: edge,
        cornerRadius: cornerRadius,
        strokeWidth: strokeWidth,
      ),
    );
  }
}

class _MorphTabActiveBorderPainter extends CustomPainter {
  const _MorphTabActiveBorderPainter({
    required this.edge,
    required this.cornerRadius,
    required this.strokeWidth,
  });

  final MorphTabActiveEdge edge;
  final double cornerRadius;
  final double strokeWidth;

  @override
  void paint(Canvas canvas, Size size) {
    final path = _edgePath(size);
    if (path == null) {
      return;
    }

    final magenta = Paint()
      ..color = MorphColors.magenta.withValues(alpha: 0.85)
      ..style = PaintingStyle.stroke
      ..strokeWidth = strokeWidth
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round
      ..isAntiAlias = true;
    final cyan = Paint()
      ..color = MorphColors.cyan
      ..style = PaintingStyle.stroke
      ..strokeWidth = strokeWidth
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round
      ..isAntiAlias = true;

    // Subtle brand offset: a short magenta segment just inside the cyan line.
    final metrics = path.computeMetrics().toList();
    if (metrics.isNotEmpty) {
      final metric = metrics.first;
      final start = math.min(12.0, metric.length * 0.15);
      final end = math.min(start + 28, metric.length * 0.55);
      if (end > start) {
        final accent = metric.extractPath(start, end);
        canvas.drawPath(accent.shift(_magentaOffset), magenta);
      }
    }
    canvas.drawPath(path, cyan);
  }

  Offset get _magentaOffset {
    switch (edge) {
      case MorphTabActiveEdge.top:
        return const Offset(0, 1.5);
      case MorphTabActiveEdge.bottom:
        return const Offset(0, -1.5);
      case MorphTabActiveEdge.left:
        return const Offset(1.5, 0);
      case MorphTabActiveEdge.right:
        return const Offset(-1.5, 0);
    }
  }

  Path? _edgePath(Size size) {
    if (size.width <= 0 || size.height <= 0) {
      return null;
    }
    final inset = strokeWidth / 2;
    final rawRadius = math.max(0.0, cornerRadius);
    final radius = math.max(0.0, rawRadius - inset);
    final path = Path();
    switch (edge) {
      case MorphTabActiveEdge.top:
        path.moveTo(inset, size.height);
        if (radius > 0) {
          path.lineTo(inset, rawRadius);
          path.arcToPoint(
            Offset(rawRadius, inset),
            radius: Radius.circular(radius),
          );
          path.lineTo(size.width - rawRadius, inset);
          path.arcToPoint(
            Offset(size.width - inset, rawRadius),
            radius: Radius.circular(radius),
          );
        } else {
          path.lineTo(inset, inset);
          path.lineTo(size.width - inset, inset);
        }
        path.lineTo(size.width - inset, size.height);
      case MorphTabActiveEdge.bottom:
        path.moveTo(inset, 0);
        if (radius > 0) {
          path.lineTo(inset, size.height - rawRadius);
          path.arcToPoint(
            Offset(rawRadius, size.height - inset),
            radius: Radius.circular(radius),
            clockwise: false,
          );
          path.lineTo(size.width - rawRadius, size.height - inset);
          path.arcToPoint(
            Offset(size.width - inset, size.height - rawRadius),
            radius: Radius.circular(radius),
            clockwise: false,
          );
        } else {
          path.lineTo(inset, size.height - inset);
          path.lineTo(size.width - inset, size.height - inset);
        }
        path.lineTo(size.width - inset, 0);
      case MorphTabActiveEdge.left:
        path.moveTo(size.width, inset);
        path.lineTo(inset, inset);
        path.lineTo(inset, size.height - inset);
        path.lineTo(size.width, size.height - inset);
      case MorphTabActiveEdge.right:
        path.moveTo(0, inset);
        path.lineTo(size.width - inset, inset);
        path.lineTo(size.width - inset, size.height - inset);
        path.lineTo(0, size.height - inset);
    }
    return path;
  }

  @override
  bool shouldRepaint(covariant _MorphTabActiveBorderPainter oldDelegate) {
    return oldDelegate.edge != edge ||
        oldDelegate.cornerRadius != cornerRadius ||
        oldDelegate.strokeWidth != strokeWidth;
  }
}
