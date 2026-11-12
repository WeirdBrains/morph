import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import '../theme/morph_theme.dart';
import '../window/morph_desktop_window.dart';
import '../window/window_buttons.dart';

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

/// Custom close control. Prefer [MorphWindowButtonsBar] for the full set.
class MorphWindowCloseButton extends StatelessWidget {
  const MorphWindowCloseButton({super.key, this.onPressed});

  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    return MorphWindowChromeButton(
      key: const Key('window-close'),
      tooltip: 'Close',
      icon: Icons.close,
      hoverColor: MorphColors.magenta,
      onPressed: onPressed ?? MorphDesktopWindow.close,
    );
  }
}

/// Minimize, maximize/restore, and close, in platform order.
///
/// Windows and Linux: minimize, maximize, close (right side of the chrome).
/// macOS fallback (when native traffic lights are not used): close, minimize,
/// zoom. Close hovers magenta; the others hover cyan.
class MorphWindowButtonsBar extends StatelessWidget {
  const MorphWindowButtonsBar({
    super.key,
    this.mode = MorphWindowButtonMode.all,
    this.platform,
    this.isMaximized = false,
    this.onMinimize,
    this.onToggleMaximize,
    this.onClose,
  });

  final MorphWindowButtonMode mode;
  final TargetPlatform? platform;
  final bool isMaximized;
  final VoidCallback? onMinimize;
  final VoidCallback? onToggleMaximize;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final target = platform ?? defaultTargetPlatform;
    final close = MorphWindowChromeButton(
      key: const Key('window-close'),
      tooltip: 'Close',
      icon: Icons.close,
      hoverColor: MorphColors.magenta,
      onPressed: onClose ?? MorphDesktopWindow.close,
    );
    if (mode == MorphWindowButtonMode.closeOnly) {
      return Row(mainAxisSize: MainAxisSize.min, children: [close]);
    }

    final minimize = MorphWindowChromeButton(
      key: const Key('window-minimize'),
      tooltip: 'Minimize',
      icon: Icons.minimize,
      hoverColor: MorphColors.cyan,
      onPressed: onMinimize ?? MorphDesktopWindow.minimize,
    );
    final maximize = MorphWindowChromeButton(
      key: const Key('window-maximize'),
      tooltip: isMaximized
          ? (target == TargetPlatform.macOS ? 'Restore' : 'Restore')
          : (target == TargetPlatform.macOS ? 'Zoom' : 'Maximize'),
      icon: isMaximized ? Icons.filter_none : Icons.crop_square,
      hoverColor: MorphColors.cyan,
      onPressed: onToggleMaximize ?? MorphDesktopWindow.toggleMaximize,
    );

    final children = target == TargetPlatform.macOS
        ? [close, minimize, maximize]
        : [minimize, maximize, close];
    return Row(mainAxisSize: MainAxisSize.min, children: children);
  }
}

/// One Glitch-styled window button.
class MorphWindowChromeButton extends StatefulWidget {
  const MorphWindowChromeButton({
    super.key,
    required this.tooltip,
    required this.icon,
    required this.hoverColor,
    required this.onPressed,
  });

  final String tooltip;
  final IconData icon;
  final Color hoverColor;
  final VoidCallback onPressed;

  @override
  State<MorphWindowChromeButton> createState() =>
      _MorphWindowChromeButtonState();
}

class _MorphWindowChromeButtonState extends State<MorphWindowChromeButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final idle = Theme.of(context).colorScheme.onSurfaceVariant;
    final color = _hover ? widget.hoverColor : idle;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: IconButton(
        tooltip: widget.tooltip,
        onPressed: widget.onPressed,
        icon: Icon(widget.icon, size: 16, color: color),
        style: IconButton.styleFrom(
          visualDensity: VisualDensity.compact,
          minimumSize: const Size(36, 36),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          padding: const EdgeInsets.all(8),
          hoverColor: widget.hoverColor.withValues(alpha: 0.16),
          highlightColor: widget.hoverColor.withValues(alpha: 0.24),
        ),
      ),
    );
  }
}
