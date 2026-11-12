import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/tabs/tab_overflow.dart';

void main() {
  const trailing = 72.0; // two 36px strip actions
  const min = MorphTabOverflow.horizontalMinExtent;
  const max = MorphTabOverflow.horizontalMaxExtent;
  const spacing = MorphTabOverflow.spacing;
  const overflowBtn = MorphTabOverflow.overflowButtonExtent;

  double needed(int tabs, {required bool overflow, required double tabExtent}) {
    var extent = tabs * tabExtent;
    if (tabs > 1) {
      extent += (tabs - 1) * spacing;
    }
    if (overflow) {
      extent += (tabs > 0 ? spacing : 0) + overflowBtn;
    }
    extent += (tabs > 0 || overflow ? spacing : 0) + trailing;
    return extent;
  }

  test('a single tab uses the 150 px maximum', () {
    expect(
      MorphTabOverflow.tabWidth(
        visibleTabCount: 1,
        available: 1280,
        trailingExtent: trailing,
        overflow: false,
      ),
      max,
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
  });

  test('many tabs clamp at the 50 px minimum before overflowing', () {
    // 20 tabs still fit at 1280 once each can shrink to 50 px.
    expect(
      MorphTabOverflow.visibleCount(
        tabCount: 20,
        available: 1280,
        minTabExtent: min,
        trailingExtent: trailing,
      ),
      20,
    );
    final packed = needed(20, overflow: false, tabExtent: min);
    expect(
      MorphTabOverflow.tabWidth(
        visibleTabCount: 20,
        available: packed,
        trailingExtent: trailing,
        overflow: false,
      ),
      closeTo(min, 0.01),
    );
    // A wider strip shares leftover pixels, but never exceeds the 150 cap
    // or drops below 50.
    final loose = MorphTabOverflow.tabWidth(
      visibleTabCount: 20,
      available: 1280,
      trailingExtent: trailing,
      overflow: false,
    );
    expect(loose, inInclusiveRange(min, max));
    expect(
      MorphTabOverflow.tabWidth(
        visibleTabCount: 40,
        available: 200,
        trailingExtent: trailing,
        overflow: true,
      ),
      min,
    );
  });

  test('overflow starts only after every visible tab is at 50 px', () {
    final allFit = needed(8, overflow: false, tabExtent: min);
    expect(
      MorphTabOverflow.visibleCount(
        tabCount: 8,
        available: allFit,
        minTabExtent: min,
        trailingExtent: trailing,
      ),
      8,
    );
    expect(
      MorphTabOverflow.visibleCount(
        tabCount: 8,
        available: allFit - 1,
        minTabExtent: min,
        trailingExtent: trailing,
      ),
      7,
    );
    expect(
      needed(7, overflow: true, tabExtent: min),
      lessThanOrEqualTo(allFit - 1 + 0.5),
    );
  });

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
    final exact = needed(4, overflow: false, tabExtent: min);
    expect(
      MorphTabOverflow.visibleCount(
        tabCount: 4,
        available: exact,
        minTabExtent: min,
        trailingExtent: trailing,
      ),
      4,
    );
    expect(
      MorphTabOverflow.tabWidth(
        visibleTabCount: 3,
        available: 1280,
        trailingExtent: trailing,
        overflow: false,
      ),
      max,
    );
  });

  test('fit calculation at a few strip widths', () {
    const widths = [580.0, 800.0, 1280.0, 1600.0];
    for (final width in widths) {
      // 40 tabs is enough that every width in this set must overflow.
      const count = 40;
      final visible = MorphTabOverflow.visibleCount(
        tabCount: count,
        available: width,
        minTabExtent: min,
        trailingExtent: trailing,
      );
      expect(visible, inInclusiveRange(1, count - 1), reason: 'width $width');
      expect(
        needed(visible, overflow: true, tabExtent: min),
        lessThanOrEqualTo(width + 0.5),
        reason: 'width $width',
      );
      expect(
        needed(visible + 1, overflow: true, tabExtent: min),
        greaterThan(width + 0.5),
        reason: 'width $width should not fit one more at 50 px',
      );
      final tabWidth = MorphTabOverflow.tabWidth(
        visibleTabCount: visible,
        available: width,
        trailingExtent: trailing,
        overflow: true,
      );
      expect(tabWidth, greaterThanOrEqualTo(min), reason: 'width $width');
      // Leftover pixels are shared, so the width sits at or just above 50.
      expect(tabWidth, lessThan(min + spacing + 1), reason: 'width $width');
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
