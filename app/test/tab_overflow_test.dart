import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/tabs/tab_overflow.dart';

void main() {
  const trailing = 72.0; // two 36px strip actions
  const min = MorphTabOverflow.horizontalMinExtent;
  const spacing = MorphTabOverflow.spacing;
  const overflowBtn = MorphTabOverflow.overflowButtonExtent;

  double needed(int tabs, {required bool overflow}) {
    var extent = tabs * min;
    if (tabs > 1) {
      extent += (tabs - 1) * spacing;
    }
    if (overflow) {
      extent += (tabs > 0 ? spacing : 0) + overflowBtn;
    }
    extent += (tabs > 0 || overflow ? spacing : 0) + trailing;
    return extent;
  }

  test('all tabs fit at wide widths without an overflow button', () {
    expect(
      MorphTabOverflow.visibleCount(
        tabCount: 3,
        available: 1280,
        minTabExtent: min,
        trailingExtent: trailing,
      ),
      3,
    );
    expect(
      MorphTabOverflow.visibleCount(
        tabCount: 1,
        available: 400,
        minTabExtent: min,
        trailingExtent: trailing,
      ),
      1,
    );
    final exact = needed(4, overflow: false);
    expect(
      MorphTabOverflow.visibleCount(
        tabCount: 4,
        available: exact,
        minTabExtent: min,
        trailingExtent: trailing,
      ),
      4,
    );
  });

  test('reserves the overflow button once tabs no longer fit', () {
    final almost = needed(4, overflow: false) - 1;
    expect(
      MorphTabOverflow.visibleCount(
        tabCount: 4,
        available: almost,
        minTabExtent: min,
        trailingExtent: trailing,
      ),
      3,
    );
    expect(
      MorphTabOverflow.visibleCount(
        tabCount: 20,
        available: 1280,
        minTabExtent: min,
        trailingExtent: trailing,
      ),
      lessThan(20),
    );
    expect(
      MorphTabOverflow.visibleCount(
        tabCount: 20,
        available: 1280,
        minTabExtent: min,
        trailingExtent: trailing,
      ),
      greaterThanOrEqualTo(1),
    );
  });

  test('fit calculation at a few strip widths', () {
    const widths = [580.0, 800.0, 1280.0, 1600.0];
    for (final width in widths) {
      final visible = MorphTabOverflow.visibleCount(
        tabCount: 20,
        available: width,
        minTabExtent: min,
        trailingExtent: trailing,
      );
      expect(visible, inInclusiveRange(1, 19), reason: 'width $width');
      expect(
        needed(visible, overflow: true),
        lessThanOrEqualTo(width + 0.5),
        reason: 'width $width',
      );
      expect(
        needed(visible + 1, overflow: true),
        greaterThan(width + 0.5),
        reason: 'width $width should not fit one more',
      );
    }
  });

  test('vertical lists use the tile height at a few heights', () {
    const tile = MorphTabOverflow.verticalMinExtent;
    for (final height in [200.0, 400.0, 720.0]) {
      final visible = MorphTabOverflow.visibleCount(
        tabCount: 20,
        available: height,
        minTabExtent: tile,
        trailingExtent: trailing,
      );
      expect(visible, inInclusiveRange(1, 19), reason: 'height $height');
    }
  });

  test('visible range keeps the active tab in the strip', () {
    expect(
      MorphTabOverflow.visibleRange(
        tabCount: 20,
        visibleCount: 5,
        activeIndex: 0,
      ),
      (0, 5),
    );
    expect(
      MorphTabOverflow.visibleRange(
        tabCount: 20,
        visibleCount: 5,
        activeIndex: 4,
      ),
      (0, 5),
    );
    expect(
      MorphTabOverflow.visibleRange(
        tabCount: 20,
        visibleCount: 5,
        activeIndex: 5,
      ),
      (1, 6),
    );
    expect(
      MorphTabOverflow.visibleRange(
        tabCount: 20,
        visibleCount: 5,
        activeIndex: 19,
      ),
      (15, 20),
    );
    expect(
      MorphTabOverflow.visibleRange(
        tabCount: 20,
        visibleCount: 20,
        activeIndex: 19,
      ),
      (0, 20),
    );
  });

  test('sidebar width clamps to 160–480 and the window', () {
    expect(MorphTabOverflow.clampSidebarWidth(220), 220);
    expect(MorphTabOverflow.clampSidebarWidth(100), 160);
    expect(MorphTabOverflow.clampSidebarWidth(800), 480);
    // 400px window: 400 - 48 - 200 = 152, raised to the 160 min.
    expect(MorphTabOverflow.clampSidebarWidth(300, windowWidth: 400), 160);
    // 800px window: 800 - 48 - 200 = 552, so the 480 cap still wins.
    expect(MorphTabOverflow.clampSidebarWidth(500, windowWidth: 800), 480);
    expect(MorphTabOverflow.clampSidebarWidth(300, windowWidth: 800), 300);
  });
}
