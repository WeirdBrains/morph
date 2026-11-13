import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/window/morph_desktop_window.dart';

void main() {
  test('only Linux leaves the title bar to the GTK runner', () {
    // nativeapi's hidden title bar on Linux wraps the content in a shadow
    // border that tiling window managers do not take away under XWayland.
    expect(
      MorphDesktopWindow.hidesTitleBarNatively(platform: TargetPlatform.linux),
      isFalse,
    );
    for (final platform in [TargetPlatform.macOS, TargetPlatform.windows]) {
      expect(
        MorphDesktopWindow.hidesTitleBarNatively(platform: platform),
        isTrue,
        reason: '$platform',
      );
    }
  });

  test('the minimum window size is one short_window_test covers', () {
    expect(MorphDesktopWindow.minimumSize, const Size(640, 300));
  });

  test('applyChromeless is a no-op without a desktop window', () {
    MorphDesktopWindow.applyChromeless();
  });
}
