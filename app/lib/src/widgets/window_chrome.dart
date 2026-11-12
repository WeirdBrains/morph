import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import '../theme/morph_theme.dart';
import '../window/morph_desktop_window.dart';

/// Edge-resize handles around the shell. Center hits fall through to [child],
/// including the web-view texture.
class MorphDesktopFrame extends StatelessWidget {
  const MorphDesktopFrame({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (!MorphDesktopWindow.isDesktop) {
      return child;
    }
    return DragToResizeArea(resizeEdgeSize: 6, child: child);
  }
}

/// Drags the window and double-click-toggles maximize (native title-bar
/// behavior). Pointers on [child] are included; place buttons and the address
/// field as siblings, not as [child], so they keep their own gestures.
class MorphWindowDragArea extends StatelessWidget {
  const MorphWindowDragArea({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (!MorphDesktopWindow.isDesktop) {
      return child;
    }
    return DragToMoveArea(child: child);
  }
}

/// Custom close control on the right of the toolbar, all desktops.
class MorphWindowCloseButton extends StatefulWidget {
  const MorphWindowCloseButton({super.key});

  @override
  State<MorphWindowCloseButton> createState() => _MorphWindowCloseButtonState();
}

class _MorphWindowCloseButtonState extends State<MorphWindowCloseButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final idle = Theme.of(context).colorScheme.onSurfaceVariant;
    final color = _hover ? MorphColors.magenta : idle;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: IconButton(
        key: const Key('window-close'),
        tooltip: 'Close',
        onPressed: MorphDesktopWindow.close,
        icon: Icon(Icons.close, size: 18, color: color),
        style: IconButton.styleFrom(
          hoverColor: MorphColors.magenta.withValues(alpha: 0.16),
          highlightColor: MorphColors.magenta.withValues(alpha: 0.24),
        ),
      ),
    );
  }
}
