import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:window_manager/window_manager.dart';

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

  /// Hide the native title bar and system caption buttons, keep the window
  /// resizable, and show content under the title bar on macOS.
  static void applyChromeless() {
    final window = current;
    if (window == null) {
      return;
    }
    try {
      window.title = 'Morph';
      window.titleBarStyle = TitleBarStyle.hidden;
      window.isWindowControlButtonsVisible = false;
      if (Window.isContentUnderTitleBarSupported()) {
        window.setContentUnderTitleBar(true);
      }
      // setFrameless last: changing the title-bar style would restore a frame.
      window.setFrameless(true);
      window.isResizable = true;
      window.isMovable = true;
      window.hasShadow = true;
      window.minimumSize = const Size(640, 480).toNative();
    } catch (_) {
      // Incomplete native backends or a test FFI load must not crash the shell.
    }
  }

  static void close() {
    current?.close();
  }
}
