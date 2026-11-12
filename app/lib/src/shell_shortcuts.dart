import 'package:flutter/services.dart';

/// Chrome-style shortcuts the Morph shell handles before a web view sees them.
enum MorphShellShortcut {
  newTab,
  closeTab,
  nextTab,
  prevTab,
  focusAddress,
  reload,
  find,
  bookmark,
  history,
  selectTab1,
  selectTab2,
  selectTab3,
  selectTab4,
  selectTab5,
  selectTab6,
  selectTab7,
  selectTab8,
  selectLastTab,
}

/// Matches Ctrl/Cmd chrome shortcuts used by the desktop (and tablet) shell.
abstract final class MorphShellShortcuts {
  /// Whether [event] is a reserved shell shortcut, including key-up so the
  /// web view never receives a dangling half of the chord.
  static bool isReserved(KeyEvent event) => match(event) != null;

  /// The shell action for [event], or `null` if the key should be forwarded.
  static MorphShellShortcut? match(KeyEvent event) {
    final key = event.logicalKey;
    if (key == LogicalKeyboardKey.controlLeft ||
        key == LogicalKeyboardKey.controlRight ||
        key == LogicalKeyboardKey.metaLeft ||
        key == LogicalKeyboardKey.metaRight ||
        key == LogicalKeyboardKey.shiftLeft ||
        key == LogicalKeyboardKey.shiftRight ||
        key == LogicalKeyboardKey.altLeft ||
        key == LogicalKeyboardKey.altRight) {
      return null;
    }

    final keyboard = HardwareKeyboard.instance;
    if (!keyboard.isControlPressed && !keyboard.isMetaPressed) {
      return null;
    }

    final shift = keyboard.isShiftPressed;

    if (key == LogicalKeyboardKey.tab) {
      return shift ? MorphShellShortcut.prevTab : MorphShellShortcut.nextTab;
    }
    if (shift) {
      return null;
    }
    if (key == LogicalKeyboardKey.keyT) {
      return MorphShellShortcut.newTab;
    }
    if (key == LogicalKeyboardKey.keyW) {
      return MorphShellShortcut.closeTab;
    }
    if (key == LogicalKeyboardKey.keyL) {
      return MorphShellShortcut.focusAddress;
    }
    if (key == LogicalKeyboardKey.keyR) {
      return MorphShellShortcut.reload;
    }
    if (key == LogicalKeyboardKey.keyF) {
      return MorphShellShortcut.find;
    }
    if (key == LogicalKeyboardKey.keyD) {
      return MorphShellShortcut.bookmark;
    }
    if (key == LogicalKeyboardKey.keyH) {
      return MorphShellShortcut.history;
    }
    if (key == LogicalKeyboardKey.digit1 || key == LogicalKeyboardKey.numpad1) {
      return MorphShellShortcut.selectTab1;
    }
    if (key == LogicalKeyboardKey.digit2 || key == LogicalKeyboardKey.numpad2) {
      return MorphShellShortcut.selectTab2;
    }
    if (key == LogicalKeyboardKey.digit3 || key == LogicalKeyboardKey.numpad3) {
      return MorphShellShortcut.selectTab3;
    }
    if (key == LogicalKeyboardKey.digit4 || key == LogicalKeyboardKey.numpad4) {
      return MorphShellShortcut.selectTab4;
    }
    if (key == LogicalKeyboardKey.digit5 || key == LogicalKeyboardKey.numpad5) {
      return MorphShellShortcut.selectTab5;
    }
    if (key == LogicalKeyboardKey.digit6 || key == LogicalKeyboardKey.numpad6) {
      return MorphShellShortcut.selectTab6;
    }
    if (key == LogicalKeyboardKey.digit7 || key == LogicalKeyboardKey.numpad7) {
      return MorphShellShortcut.selectTab7;
    }
    if (key == LogicalKeyboardKey.digit8 || key == LogicalKeyboardKey.numpad8) {
      return MorphShellShortcut.selectTab8;
    }
    if (key == LogicalKeyboardKey.digit9 || key == LogicalKeyboardKey.numpad9) {
      return MorphShellShortcut.selectLastTab;
    }
    return null;
  }

  /// 0-based tab index for a [MorphShellShortcut.selectTab*] action.
  static int? tabIndex(MorphShellShortcut shortcut) {
    return switch (shortcut) {
      MorphShellShortcut.selectTab1 => 0,
      MorphShellShortcut.selectTab2 => 1,
      MorphShellShortcut.selectTab3 => 2,
      MorphShellShortcut.selectTab4 => 3,
      MorphShellShortcut.selectTab5 => 4,
      MorphShellShortcut.selectTab6 => 5,
      MorphShellShortcut.selectTab7 => 6,
      MorphShellShortcut.selectTab8 => 7,
      MorphShellShortcut.selectLastTab => -1,
      _ => null,
    };
  }
}
