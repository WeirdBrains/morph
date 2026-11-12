import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';

/// Navigation and load state reported by a backend.
class MorphNavigationEvent {
  const MorphNavigationEvent({
    required this.url,
    this.title = '',
    this.canGoBack = false,
    this.canGoForward = false,
    this.isLoading = false,
    this.progress = 0,
  });

  final String url;
  final String title;
  final bool canGoBack;
  final bool canGoForward;
  final bool isLoading;

  /// 0.0–1.0
  final double progress;
}

/// Which texture path a Linux view is using, plus the detection reason.
class MorphRenderPathInfo {
  const MorphRenderPathInfo({required this.path, required this.reason});

  /// `wpe-dmabuf`, `flutter-gl`, or `webkitgtk-snapshot`.
  final String path;
  final String reason;

  bool get isDmaBuf => path == 'wpe-dmabuf';
  bool get isGlTexture => path == 'flutter-gl';
  bool get isSnapshot => path == 'webkitgtk-snapshot';

  @override
  String toString() => '$path ($reason)';
}

/// A pointer, hover, or scroll event in view-logical coordinates
/// (device-pixel-ratio applied by the backend).
class MorphPointerEvent {
  const MorphPointerEvent({
    required this.phase,
    required this.x,
    required this.y,
    this.button = 0,
    this.buttons = 0,
    this.scrollDeltaX = 0,
    this.scrollDeltaY = 0,
    this.ctrl = false,
    this.alt = false,
    this.shift = false,
    this.meta = false,
  });

  /// down, move, up, cancel, scroll, enter, leave, hover
  final String phase;
  final double x;
  final double y;

  /// The button that changed for down/up: 1=left, 2=middle, 3=right, 4=back,
  /// 5=forward; 0 otherwise.
  final int button;

  /// Mask of buttons currently held: 1=left, 2=middle, 4=right, 8=back,
  /// 16=forward.
  final int buttons;
  final double scrollDeltaX;
  final double scrollDeltaY;
  final bool ctrl;
  final bool alt;
  final bool shift;
  final bool meta;

  Map<String, Object?> toMap() => {
    'phase': phase,
    'x': x,
    'y': y,
    'button': button,
    'buttons': buttons,
    'scrollDeltaX': scrollDeltaX,
    'scrollDeltaY': scrollDeltaY,
    'ctrl': ctrl,
    'alt': alt,
    'shift': shift,
    'meta': meta,
  };
}

/// Convert a Flutter button mask ([kPrimaryMouseButton], etc.) to Morph's
/// `buttons` bitfield.
int morphButtonsFromFlutter(int flutterButtons) {
  var mask = 0;
  if (flutterButtons & kPrimaryMouseButton != 0) {
    mask |= 1;
  }
  if (flutterButtons & kMiddleMouseButton != 0) {
    mask |= 2;
  }
  if (flutterButtons & kSecondaryMouseButton != 0) {
    mask |= 4;
  }
  if (flutterButtons & kBackMouseButton != 0) {
    mask |= 8;
  }
  if (flutterButtons & kForwardMouseButton != 0) {
    mask |= 16;
  }
  return mask;
}

/// The first Morph `button` id set in a Morph buttons mask, or 0.
int morphButtonFromMask(int mask) {
  if (mask & 1 != 0) {
    return 1;
  }
  if (mask & 2 != 0) {
    return 2;
  }
  if (mask & 4 != 0) {
    return 3;
  }
  if (mask & 8 != 0) {
    return 4;
  }
  if (mask & 16 != 0) {
    return 5;
  }
  return 0;
}

bool morphModifierPressed({
  required LogicalKeyboardKey left,
  required LogicalKeyboardKey right,
}) {
  return HardwareKeyboard.instance.logicalKeysPressed.contains(left) ||
      HardwareKeyboard.instance.logicalKeysPressed.contains(right);
}

/// Stable control-layer key name for a Flutter logical key.
String morphKeyName(LogicalKeyboardKey key) {
  if (key == LogicalKeyboardKey.backspace) {
    return 'Backspace';
  }
  if (key == LogicalKeyboardKey.enter ||
      key == LogicalKeyboardKey.numpadEnter) {
    return 'Enter';
  }
  if (key == LogicalKeyboardKey.tab) {
    return 'Tab';
  }
  if (key == LogicalKeyboardKey.escape) {
    return 'Escape';
  }
  if (key == LogicalKeyboardKey.arrowLeft) {
    return 'Arrow Left';
  }
  if (key == LogicalKeyboardKey.arrowRight) {
    return 'Arrow Right';
  }
  if (key == LogicalKeyboardKey.arrowUp) {
    return 'Arrow Up';
  }
  if (key == LogicalKeyboardKey.arrowDown) {
    return 'Arrow Down';
  }
  if (key == LogicalKeyboardKey.delete) {
    return 'Delete';
  }
  if (key == LogicalKeyboardKey.home) {
    return 'Home';
  }
  if (key == LogicalKeyboardKey.end) {
    return 'End';
  }
  if (key == LogicalKeyboardKey.pageUp) {
    return 'Page Up';
  }
  if (key == LogicalKeyboardKey.pageDown) {
    return 'Page Down';
  }
  if (key == LogicalKeyboardKey.space) {
    return 'Space';
  }
  if (key == LogicalKeyboardKey.insert) {
    return 'Insert';
  }
  if (key.keyLabel.isNotEmpty) {
    return key.keyLabel;
  }
  return key.debugName ?? key.keyId.toString();
}

/// Produced character for a key event, or empty for control keys.
///
/// Flutter's Linux embedder sets [KeyEvent.character] from
/// `gdk_keyval_to_unicode()`, so Backspace is U+0008 and Enter is U+000D.
/// Those must not be forwarded as `text` or a backend may treat them as
/// Unicode keyvals instead of GDK_KEY_BackSpace / GDK_KEY_Return.
String morphKeyText(KeyEvent event) {
  final character = event.character;
  if (character == null || character.isEmpty) {
    return '';
  }
  final ch = character.runes.first;
  if (ch < 0x20 || ch == 0x7f) {
    return '';
  }
  return character;
}

class MorphKeyEvent {
  const MorphKeyEvent({
    required this.phase,
    required this.key,
    this.hardwareKeycode = 0,
    this.ctrl = false,
    this.alt = false,
    this.shift = false,
    this.meta = false,
    this.text = '',
    this.repeat = false,
  });

  /// down or up
  final String phase;
  final String key;
  final int hardwareKeycode;
  final bool ctrl;
  final bool alt;
  final bool shift;
  final bool meta;

  /// Produced character when there is one (e.g. `a`, `A`).
  final String text;
  final bool repeat;

  Map<String, Object?> toMap() => {
    'phase': phase,
    'key': key,
    'hardwareKeycode': hardwareKeycode,
    'ctrl': ctrl,
    'alt': alt,
    'shift': shift,
    'meta': meta,
    'text': text,
    'repeat': repeat,
  };
}

class MorphImeEvent {
  const MorphImeEvent({required this.phase, this.text = ''});

  /// compose_start, compose_update, compose_end, commit, focus, blur
  final String phase;
  final String text;
}

/// A CSS cursor keyword reported by the web view.
class MorphCursorEvent {
  const MorphCursorEvent({required this.shape});

  final String shape;
}

/// Map a CSS (or GDK) cursor name onto a Flutter [MouseCursor].
MouseCursor morphCursorForShape(String shape) {
  switch (shape) {
    case 'pointer':
    case 'hand':
    case 'hand1':
    case 'hand2':
      return SystemMouseCursors.click;
    case 'text':
    case 'xterm':
    case 'ibeam':
      return SystemMouseCursors.text;
    case 'vertical-text':
      return SystemMouseCursors.verticalText;
    case 'wait':
    case 'watch':
      return SystemMouseCursors.wait;
    case 'progress':
    case 'left_ptr_watch':
      return SystemMouseCursors.progress;
    case 'crosshair':
    case 'cross':
    case 'tcross':
      return SystemMouseCursors.precise;
    case 'move':
    case 'fleur':
    case 'all-scroll':
      return SystemMouseCursors.move;
    case 'not-allowed':
    case 'no-drop':
    case 'crossed_circle':
      return SystemMouseCursors.forbidden;
    case 'grab':
    case 'openhand':
    case 'grabbing':
    case 'closedhand':
      return shape == 'grabbing' || shape == 'closedhand'
          ? SystemMouseCursors.grabbing
          : SystemMouseCursors.grab;
    case 'help':
    case 'question_arrow':
      return SystemMouseCursors.help;
    case 'none':
      return SystemMouseCursors.none;
    case 'cell':
    case 'plus':
      return SystemMouseCursors.cell;
    case 'context-menu':
      return SystemMouseCursors.contextMenu;
    case 'alias':
    case 'dnd-link':
      return SystemMouseCursors.alias;
    case 'copy':
    case 'dnd-copy':
      return SystemMouseCursors.copy;
    case 'zoom-in':
      return SystemMouseCursors.zoomIn;
    case 'zoom-out':
      return SystemMouseCursors.zoomOut;
    case 'col-resize':
    case 'sb_h_double_arrow':
      return SystemMouseCursors.resizeColumn;
    case 'row-resize':
    case 'sb_v_double_arrow':
      return SystemMouseCursors.resizeRow;
    case 'n-resize':
    case 'top_side':
      return SystemMouseCursors.resizeUp;
    case 's-resize':
    case 'bottom_side':
      return SystemMouseCursors.resizeDown;
    case 'e-resize':
    case 'right_side':
      return SystemMouseCursors.resizeRight;
    case 'w-resize':
    case 'left_side':
      return SystemMouseCursors.resizeLeft;
    case 'ne-resize':
    case 'top_right_corner':
      return SystemMouseCursors.resizeUpRight;
    case 'nw-resize':
    case 'top_left_corner':
      return SystemMouseCursors.resizeUpLeft;
    case 'se-resize':
    case 'bottom_right_corner':
      return SystemMouseCursors.resizeDownRight;
    case 'sw-resize':
    case 'bottom_left_corner':
      return SystemMouseCursors.resizeDownLeft;
    case 'ew-resize':
    case 'sb_h_double_arrow_ew':
      return SystemMouseCursors.resizeLeftRight;
    case 'ns-resize':
      return SystemMouseCursors.resizeUpDown;
    case 'nesw-resize':
      return SystemMouseCursors.resizeUpRightDownLeft;
    case 'nwse-resize':
      return SystemMouseCursors.resizeUpLeftDownRight;
    case 'default':
    case 'auto':
    case 'left_ptr':
    case 'arrow':
    default:
      return SystemMouseCursors.basic;
  }
}

/// Pretty-print a pointer event in debug builds.
String debugFormatPointer(MorphPointerEvent event) {
  if (kDebugMode) {
    return '${event.phase} (${event.x}, ${event.y}) btn=${event.button} mask=${event.buttons}';
  }
  return event.phase;
}
