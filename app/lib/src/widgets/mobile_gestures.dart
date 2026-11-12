import 'package:flutter/material.dart';

import '../theme/morph_theme.dart';

/// Mobile navigation gestures over the page: edge-swipe back and forward
/// with a visual arrow, and pull-to-refresh.
///
/// Edge overlays are only 28 logical pixels wide so they do not steal
/// mid-page horizontal pans. Pull-to-refresh is recognized when a downward
/// drag starts in the top 48 pixels of the page.
class MorphMobileGestures extends StatefulWidget {
  const MorphMobileGestures({
    super.key,
    required this.child,
    this.enabled = true,
    this.canGoBack = false,
    this.canGoForward = false,
    this.canRefresh = false,
    this.onBack,
    this.onForward,
    this.onRefresh,
  });

  final Widget child;
  final bool enabled;
  final bool canGoBack;
  final bool canGoForward;
  final bool canRefresh;
  final VoidCallback? onBack;
  final VoidCallback? onForward;
  final VoidCallback? onRefresh;

  static const edgeWidth = 28.0;
  static const pullZone = 48.0;
  static const swipeCommit = 56.0;
  static const pullCommit = 64.0;

  @override
  State<MorphMobileGestures> createState() => _MorphMobileGesturesState();
}

class _MorphMobileGesturesState extends State<MorphMobileGestures> {
  _GestureKind? _kind;
  double _offset = 0;

  bool get _showBack => _kind == _GestureKind.back && _offset > 8;
  bool get _showForward => _kind == _GestureKind.forward && _offset > 8;
  bool get _showRefresh => _kind == _GestureKind.refresh && _offset > 8;

  void _end() {
    final offset = _offset;
    final kind = _kind;
    setState(() {
      _kind = null;
      _offset = 0;
    });
    if (kind == _GestureKind.back &&
        offset >= MorphMobileGestures.swipeCommit &&
        widget.canGoBack) {
      widget.onBack?.call();
    } else if (kind == _GestureKind.forward &&
        offset >= MorphMobileGestures.swipeCommit &&
        widget.canGoForward) {
      widget.onForward?.call();
    } else if (kind == _GestureKind.refresh &&
        offset >= MorphMobileGestures.pullCommit &&
        widget.canRefresh) {
      widget.onRefresh?.call();
    }
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.enabled) {
      return widget.child;
    }
    return Stack(
      key: const Key('mobile-gestures'),
      fit: StackFit.expand,
      children: [
        widget.child,
        // Left-edge back swipe.
        Positioned(
          left: 0,
          top: 0,
          bottom: 0,
          width: MorphMobileGestures.edgeWidth,
          child: GestureDetector(
            key: const Key('gesture-back-edge'),
            behavior: HitTestBehavior.translucent,
            onHorizontalDragStart: (_) {
              if (widget.canGoBack) {
                setState(() {
                  _kind = _GestureKind.back;
                  _offset = 0;
                });
              }
            },
            onHorizontalDragUpdate: (details) {
              if (_kind != _GestureKind.back) {
                return;
              }
              setState(() {
                _offset = (_offset + details.delta.dx).clamp(0, 120);
              });
            },
            onHorizontalDragEnd: (_) => _end(),
            onHorizontalDragCancel: _end,
          ),
        ),
        // Right-edge forward swipe.
        Positioned(
          right: 0,
          top: 0,
          bottom: 0,
          width: MorphMobileGestures.edgeWidth,
          child: GestureDetector(
            key: const Key('gesture-forward-edge'),
            behavior: HitTestBehavior.translucent,
            onHorizontalDragStart: (_) {
              if (widget.canGoForward) {
                setState(() {
                  _kind = _GestureKind.forward;
                  _offset = 0;
                });
              }
            },
            onHorizontalDragUpdate: (details) {
              if (_kind != _GestureKind.forward) {
                return;
              }
              setState(() {
                _offset = (_offset - details.delta.dx).clamp(0, 120);
              });
            },
            onHorizontalDragEnd: (_) => _end(),
            onHorizontalDragCancel: _end,
          ),
        ),
        // Pull-to-refresh from the top strip.
        Positioned(
          left: MorphMobileGestures.edgeWidth,
          right: MorphMobileGestures.edgeWidth,
          top: 0,
          height: MorphMobileGestures.pullZone,
          child: GestureDetector(
            key: const Key('gesture-refresh-edge'),
            behavior: HitTestBehavior.translucent,
            onVerticalDragStart: (_) {
              if (widget.canRefresh) {
                setState(() {
                  _kind = _GestureKind.refresh;
                  _offset = 0;
                });
              }
            },
            onVerticalDragUpdate: (details) {
              if (_kind != _GestureKind.refresh) {
                return;
              }
              setState(() {
                _offset = (_offset + details.delta.dy).clamp(0, 140);
              });
            },
            onVerticalDragEnd: (_) => _end(),
            onVerticalDragCancel: _end,
          ),
        ),
        if (_showBack)
          _ArrowCue(
            key: const Key('gesture-back-cue'),
            alignment: Alignment.centerLeft,
            offset: _offset,
            icon: Icons.arrow_back,
            ready: _offset >= MorphMobileGestures.swipeCommit,
          ),
        if (_showForward)
          _ArrowCue(
            key: const Key('gesture-forward-cue'),
            alignment: Alignment.centerRight,
            offset: _offset,
            icon: Icons.arrow_forward,
            ready: _offset >= MorphMobileGestures.swipeCommit,
          ),
        if (_showRefresh)
          Positioned(
            key: const Key('gesture-refresh-cue'),
            top: 8 + _offset.clamp(0, 72) / 2,
            left: 0,
            right: 0,
            child: Center(
              child: _CueDisc(
                icon: Icons.refresh,
                ready: _offset >= MorphMobileGestures.pullCommit,
              ),
            ),
          ),
      ],
    );
  }
}

enum _GestureKind { back, forward, refresh }

class _ArrowCue extends StatelessWidget {
  const _ArrowCue({
    super.key,
    required this.alignment,
    required this.offset,
    required this.icon,
    required this.ready,
  });

  final Alignment alignment;
  final double offset;
  final IconData icon;
  final bool ready;

  @override
  Widget build(BuildContext context) {
    final dx = alignment == Alignment.centerLeft ? offset : -offset;
    return Align(
      alignment: alignment,
      child: Transform.translate(
        offset: Offset(dx, 0),
        child: _CueDisc(icon: icon, ready: ready),
      ),
    );
  }
}

class _CueDisc extends StatelessWidget {
  const _CueDisc({required this.icon, required this.ready});

  final IconData icon;
  final bool ready;

  @override
  Widget build(BuildContext context) {
    final color = ready ? MorphColors.cyan : MorphColors.magenta;
    return Container(
      width: 40,
      height: 40,
      decoration: BoxDecoration(
        shape: BoxShape.circle,
        color: MorphColors.charcoal.withValues(alpha: 0.88),
        border: Border.all(color: color, width: 2),
        boxShadow: [
          BoxShadow(color: color.withValues(alpha: 0.4), blurRadius: 8),
        ],
      ),
      child: Icon(icon, color: color, size: 20),
    );
  }
}
