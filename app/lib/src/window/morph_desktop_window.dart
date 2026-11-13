import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:window_manager/window_manager.dart';

import 'window_buttons.dart';

/// Native window controls for the desktop shells (Linux, macOS, Windows).
///
/// No-ops in widget tests and on mobile: [WindowManager.instance.getCurrent]
/// is null without a real desktop window, and [isDesktop] is false when the
/// binding reports Android or iOS (the test default).
class MorphDesktopWindow {
  MorphDesktopWindow._();

  static bool get isDesktop {
    if (kIsWeb) {
      return false;
    }
    switch (defaultTargetPlatform) {
      case TargetPlatform.linux:
      case TargetPlatform.macOS:
      case TargetPlatform.windows:
        return true;
      default:
        return false;
    }
  }

  static Window? get current {
    if (!isDesktop) {
      return null;
    }
    try {
      return WindowManager.instance.getCurrent();
    } catch (_) {
      return null;
    }
  }

  /// Whether the shell should keep native macOS traffic lights visible
  /// under a hidden title bar, instead of drawing custom buttons.
  static bool get usesNativeMacTrafficLights =>
      isDesktop && defaultTargetPlatform == TargetPlatform.macOS;

  /// The smallest window Morph asks for.
  static const minimumSize = Size(640, 300);

  /// Whether [applyChromeless] asks window_manager to hide the title bar.
  ///
  /// Not on Linux: the GTK runner already makes the window undecorated, and
  /// window_manager's hidden title bar there (nativeapi) puts its own shadow
  /// in a transparent border around the content, about 180 px a side. It
  /// leaves the border out only when the window manager reports the window
  /// tiled, maximized or full screen. Through XWayland, Hyprland reports none
  /// of these but still fits the whole window to its tile, so Morph drew in
  /// a small strip in the middle of a blank frame (2026-11-13).
  static bool hidesTitleBarNatively({TargetPlatform? platform}) =>
      (platform ?? defaultTargetPlatform) != TargetPlatform.linux;

  /// Hide the native title bar. On macOS the traffic lights stay visible and
  /// content draws under the title bar so the tab strip can pad around them.
  /// On Windows the frame is removed; on Linux the runner never adds one
  /// (see [hidesTitleBarNatively]). Morph draws its own buttons on both.
  static void applyChromeless() {
    final window = current;
    if (window == null) {
      return;
    }
    try {
      window.title = 'Morph';
      if (hidesTitleBarNatively()) {
        window.titleBarStyle = TitleBarStyle.hidden;
        final isMac = defaultTargetPlatform == TargetPlatform.macOS;
        if (isMac) {
          window.isWindowControlButtonsVisible = true;
          if (Window.isContentUnderTitleBarSupported()) {
            window.setContentUnderTitleBar(true);
          }
        } else {
          window.isWindowControlButtonsVisible = false;
          // setFrameless last: changing the title-bar style would restore a
          // frame.
          window.setFrameless(true);
        }
        window.hasShadow = true;
      }
      window.isResizable = true;
      window.isMovable = true;
      // Tiling window managers give windows shorter than 480 px (Hyprland's
      // half tiles on a laptop); a taller minimum makes GTK lay out past the
      // bottom of the window. The shell and dialogs fit 640x300
      // (short_window_test).
      window.minimumSize = minimumSize.toNative();
    } catch (_) {
      // Incomplete native backends or a test FFI load must not crash the shell.
    }
  }

  static void close() {
    current?.close();
  }

  static void minimize() {
    current?.minimize();
  }

  static void maximize() {
    current?.maximize();
  }

  static void unmaximize() {
    current?.unmaximize();
  }

  static void toggleMaximize() {
    final window = current;
    if (window == null) {
      return;
    }
    if (window.isMaximized) {
      window.unmaximize();
    } else {
      window.maximize();
    }
  }

  static bool get isMaximized => current?.isMaximized ?? false;

  /// Right edge of the native zoom button, if the backend can measure it.
  static double? measuredMacTrafficLightRight() {
    try {
      final window = current;
      if (window == null || !Window.isMaximizeButtonBoundsSupported()) {
        return null;
      }
      final bounds = window.maximizeButtonBounds;
      final right = bounds.x + bounds.width;
      if (right > 40 && right < 160) {
        return right;
      }
    } catch (_) {
      // Missing or stubbed backends must fall back to the default inset.
    }
    return null;
  }

  /// Leading padding so the mark and tabs clear native macOS traffic lights.
  static double leadingInset({
    TargetPlatform? platform,
    bool useNativeMacButtons = true,
  }) {
    return MorphWindowChrome.leadingInset(
      platform: platform,
      useNativeMacButtons: useNativeMacButtons,
      measuredTrafficLightRight: measuredMacTrafficLightRight(),
    );
  }
}
