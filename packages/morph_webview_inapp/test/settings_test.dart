import 'package:flutter_inappwebview/flutter_inappwebview.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph_webview_inapp/src/settings.dart';

void main() {
  test('Android uses texture-layer composition without a hardware layer', () {
    final settings = morphInAppWebViewSettings();
    // Hybrid composition plus LAYER_TYPE_HARDWARE leaves a white, frozen
    // surface after the keyboard or a second page load on Flutter 3.47.
    expect(settings.useHybridComposition, isFalse);
    expect(settings.hardwareAcceleration, isFalse);
    expect(settings.useOnRenderProcessGone, isTrue);
  });

  test('desktop-site toggles keep the composition settings', () {
    for (final desktop in [true, false]) {
      final settings = morphInAppWebViewSettings(desktopSite: desktop);
      expect(settings.useHybridComposition, isFalse);
      expect(settings.hardwareAcceleration, isFalse);
      expect(settings.useOnRenderProcessGone, isTrue);
      expect(settings.useWideViewPort, desktop);
      expect(settings.loadWithOverviewMode, desktop);
      expect(
        settings.preferredContentMode,
        desktop
            ? UserPreferredContentMode.DESKTOP
            : UserPreferredContentMode.MOBILE,
      );
      final map = settings.toMap();
      expect(map['useHybridComposition'], isFalse);
      expect(map['hardwareAcceleration'], isFalse);
    }
  });

  test('creation keeps each platform recommended content mode', () {
    final settings = morphInAppWebViewSettings();
    expect(settings.preferredContentMode, UserPreferredContentMode.RECOMMENDED);
    expect(settings.useWideViewPort, isTrue);
    expect(settings.loadWithOverviewMode, isTrue);
  });

  test('profile storage stays persistent', () {
    final settings = morphInAppWebViewSettings();
    expect(settings.incognito, isFalse);
    expect(settings.cacheEnabled, isTrue);
    expect(settings.javaScriptEnabled, isTrue);
  });

  test('pages can open real windows (sign-in popups keep their opener)', () {
    for (final desktop in [null, true, false]) {
      final settings = morphInAppWebViewSettings(desktopSite: desktop);
      expect(settings.supportMultipleWindows, isTrue);
      expect(settings.javaScriptCanOpenWindowsAutomatically, isTrue);
    }
  });
}
