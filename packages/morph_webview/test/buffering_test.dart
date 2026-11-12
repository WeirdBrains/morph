import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph_webview/morph_webview.dart';

void main() {
  late FakeMorphWebViewPlatform fake;

  setUp(() {
    fake = FakeMorphWebViewPlatform();
    MorphWebViewPlatform.instance = fake;
  });

  testWidgets('a hidden tab goes idle only after idleAfter, and comes back '
      'at once when shown', (tester) async {
    final controller = MorphWebViewController(
      initialUrl: 'about:blank',
      idleAfter: const Duration(seconds: 30),
    );
    await tester.runAsync(
      () => controller.attach(width: 400, height: 800, devicePixelRatio: 2),
    );
    final session = fake.lastSession!;
    expect(controller.buffering, MorphBuffering.full);

    controller.setVisible(false);
    await tester.pump(const Duration(seconds: 29));
    expect(session.bufferingChanges, isEmpty, reason: 'brief hides keep it');
    controller.setVisible(true);
    await tester.pump(const Duration(seconds: 60));
    expect(session.bufferingChanges, isEmpty, reason: 'timer was cancelled');

    controller.setVisible(false);
    controller.setVisible(false); // repeated hides do not restart the clock
    await tester.pump(const Duration(seconds: 30));
    expect(session.bufferingChanges, [MorphBuffering.idle]);
    expect(controller.buffering, MorphBuffering.idle);

    controller.setVisible(true);
    expect(session.bufferingChanges, [
      MorphBuffering.idle,
      MorphBuffering.full,
    ], reason: 'restored synchronously, before the switch is drawn');
    controller.setVisible(true);
    expect(session.bufferingChanges, hasLength(2));
    await tester.runAsync(controller.dispose);
  });

  testWidgets('a tab that went idle before its view existed starts idle', (
    tester,
  ) async {
    final controller = MorphWebViewController(
      initialUrl: 'about:blank',
      idleAfter: const Duration(seconds: 5),
    );
    controller.setVisible(false);
    await tester.pump(const Duration(seconds: 5));
    await tester.runAsync(
      () => controller.attach(width: 400, height: 800, devicePixelRatio: 2),
    );
    expect(fake.lastSession!.bufferingChanges, [MorphBuffering.idle]);
    await tester.runAsync(controller.dispose);
  });

  testWidgets('disposing a hidden tab cancels its idle timer', (tester) async {
    final controller = MorphWebViewController(initialUrl: 'about:blank');
    controller.setVisible(false);
    await tester.runAsync(controller.dispose);
    // No pending timer is left behind (the test binding fails otherwise).
    await tester.pump(MorphWebViewController.defaultIdleAfter);
  });

  testWidgets('MorphWebView tells its controller whether it is on screen', (
    tester,
  ) async {
    final controller = MorphWebViewController(initialUrl: 'about:blank');
    Widget view(bool active) => Directionality(
      textDirection: TextDirection.ltr,
      child: MorphWebView(
        controller: controller,
        active: active,
        placeholder: const SizedBox(),
      ),
    );
    await tester.pumpWidget(view(true));
    expect(controller.visible, isTrue);
    await tester.pumpWidget(view(false));
    expect(controller.visible, isFalse);
    await tester.pumpWidget(view(true));
    expect(controller.visible, isTrue);
    await tester.pumpWidget(const SizedBox());
    await tester.runAsync(controller.dispose);
  });
}
