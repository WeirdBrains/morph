/// Fit math for the desktop tab strip and list overflow menu.
///
/// The strip shows as many tabs as will fit at [horizontalMinExtent] (or
/// [verticalMinExtent] in a list), after reserving the trailing +/position
/// group and, when needed, an overflow button.
class MorphTabOverflow {
  MorphTabOverflow._();

  /// Minimum width of a horizontal tab chip.
  static const double horizontalMinExtent = 180;

  /// Height of a left/right tab list tile.
  static const double verticalMinExtent = 36;

  /// Size of the overflow chevron button (matches the other strip actions).
  static const double overflowButtonExtent = 36;

  /// Size of the + and tab-position actions.
  static const double trailingActionExtent = 36;

  /// Default expanded width of the left/right tab list.
  static const double defaultSidebarWidth = 220;

  /// Narrowest expanded sidebar width the user can drag to.
  static const double minSidebarWidth = 160;

  /// Widest expanded sidebar width the user can drag to.
  static const double maxSidebarWidth = 480;

  /// Width of the collapsed favicon rail.
  static const double collapsedSidebarWidth = 48;

  /// Gap between tabs, the overflow button, and the trailing actions.
  static const double spacing = 2;

  /// Extent of the + and position action group along [horizontal] axis.
  static double trailingGroupExtent({required bool horizontal}) {
    if (horizontal) {
      return trailingActionExtent * 2;
    }
    return trailingActionExtent * 2;
  }

  /// How many tabs to show in a strip or list of [available] extent.
  ///
  /// When every tab fits without an overflow button, the result is [tabCount].
  /// Otherwise the overflow button is reserved and at least one tab stays
  /// hidden. A single tab is always shown, even if the chrome is cramped.
  static int visibleCount({
    required int tabCount,
    required double available,
    required double minTabExtent,
    required double trailingExtent,
    double overflowButtonExtent = overflowButtonExtent,
    double spacing = spacing,
  }) {
    if (tabCount <= 0) {
      return 0;
    }

    double extentFor(int tabs, {required bool overflow}) {
      var extent = tabs * minTabExtent;
      if (tabs > 1) {
        extent += (tabs - 1) * spacing;
      }
      if (overflow) {
        extent += (tabs > 0 ? spacing : 0) + overflowButtonExtent;
      }
      if (trailingExtent > 0) {
        extent += (tabs > 0 || overflow ? spacing : 0) + trailingExtent;
      }
      return extent;
    }

    if (extentFor(tabCount, overflow: false) <= available + 0.5) {
      return tabCount;
    }
    var visible = tabCount - 1;
    while (visible > 1 &&
        extentFor(visible, overflow: true) > available + 0.5) {
      visible--;
    }
    if (visible < 1) {
      visible = 1;
    }
    return visible;
  }

  /// Inclusive-start, exclusive-end window of visible tabs that always
  /// contains [activeIndex]. When the active tab would be hidden, the window
  /// slides so that tab becomes the last visible entry.
  static (int start, int end) visibleRange({
    required int tabCount,
    required int visibleCount,
    required int activeIndex,
  }) {
    if (tabCount <= 0 || visibleCount <= 0) {
      return (0, 0);
    }
    final cap = visibleCount > tabCount ? tabCount : visibleCount;
    if (cap >= tabCount) {
      return (0, tabCount);
    }
    var active = activeIndex;
    if (active < 0) {
      active = 0;
    } else if (active >= tabCount) {
      active = tabCount - 1;
    }
    var start = 0;
    if (active >= cap) {
      start = active - cap + 1;
    }
    final maxStart = tabCount - cap;
    if (start > maxStart) {
      start = maxStart;
    }
    return (start, start + cap);
  }

  /// Clamp an expanded sidebar width into [minSidebarWidth], [maxSidebarWidth],
  /// and the space a window of [windowWidth] can actually spare.
  static double clampSidebarWidth(double width, {double? windowWidth}) {
    var max = maxSidebarWidth;
    if (windowWidth != null && windowWidth.isFinite && windowWidth > 0) {
      // Leave room for the nav/content and a collapsed rail on the far side.
      final room = windowWidth - collapsedSidebarWidth - 200;
      if (room < max) {
        max = room;
      }
    }
    if (max < minSidebarWidth) {
      max = minSidebarWidth;
    }
    if (width < minSidebarWidth) {
      return minSidebarWidth;
    }
    if (width > max) {
      return max;
    }
    return width;
  }
}
