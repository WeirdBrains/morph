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

  /// `wpe-dmabuf` or `webkitgtk-snapshot`.
  final String path;
  final String reason;

  bool get isDmaBuf => path == 'wpe-dmabuf';
  bool get isSnapshot => path == 'webkitgtk-snapshot';

  @override
  String toString() => '$path ($reason)';
}

/// A pointer or scroll event in view-logical coordinates (device-pixel-ratio applied by the backend).
class MorphPointerEvent {
  const MorphPointerEvent({
    required this.phase,
    required this.x,
    required this.y,
    this.button = 0,
    this.scrollDeltaX = 0,
    this.scrollDeltaY = 0,
  });

  /// down, move, up, cancel, scroll
  final String phase;
  final double x;
  final double y;

  /// 1 = left, 2 = middle, 3 = right; 0 for move/scroll.
  final int button;
  final double scrollDeltaX;
  final double scrollDeltaY;

  Map<String, Object?> toMap() => {
    'phase': phase,
    'x': x,
    'y': y,
    'button': button,
    'scrollDeltaX': scrollDeltaX,
    'scrollDeltaY': scrollDeltaY,
  };
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
  });

  /// down or up
  final String phase;
  final String key;
  final int hardwareKeycode;
  final bool ctrl;
  final bool alt;
  final bool shift;
  final bool meta;

  Map<String, Object?> toMap() => {
    'phase': phase,
    'key': key,
    'hardwareKeycode': hardwareKeycode,
    'ctrl': ctrl,
    'alt': alt,
    'shift': shift,
    'meta': meta,
  };
}
