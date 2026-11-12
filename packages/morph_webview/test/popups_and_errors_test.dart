import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph_webview/morph_webview.dart';

Future<void> _pump() async {
  await Future<void>.delayed(Duration.zero);
  await Future<void>.delayed(Duration.zero);
}

void main() {
  late FakeMorphWebViewPlatform fake;

  setUp(() {
    fake = FakeMorphWebViewPlatform();
    MorphWebViewPlatform.instance = fake;
  });

  Future<(MorphWebViewController, FakeMorphWebViewSession)> attached({
    Duration grace = MorphWebViewController.defaultUnresponsiveGrace,
  }) async {
    final controller = MorphWebViewController(
      initialUrl: 'https://example.com',
      unresponsiveGrace: grace,
    );
    await controller.attach(width: 800, height: 600, devicePixelRatio: 1);
    return (controller, fake.lastSession!);
  }

  group('popups', () {
    test('window.open stacks a popup that close returns from', () async {
      final (controller, session) = await attached();
      final first = session.emitPopup('https://accounts.example/login');
      await _pump();
      expect(controller.popups.value, hasLength(1));
      final popup = controller.popups.value.single;
      expect(popup.isAttached, isTrue);
      first.emitCloseRequested();
      await _pump();
      expect(controller.popups.value, isEmpty);
      expect(first.disposed, isTrue);
      expect(session.disposed, isFalse);
      await controller.dispose();
    });

    test('at most four popups stack; a fifth is refused', () async {
      final (controller, session) = await attached();
      final opened = [
        for (var i = 0; i < 5; i++) session.emitPopup('https://p$i.example'),
      ];
      await _pump();
      expect(
        controller.popups.value,
        hasLength(MorphWebViewController.maxPopups),
      );
      expect(opened.last.disposed, isTrue);
      await controller.dispose();
      expect(opened.take(4).every((s) => s.disposed), isTrue);
    });

    test('a popup opened by a popup stacks on the same tab', () async {
      final (controller, session) = await attached();
      final first = session.emitPopup('https://a.example');
      await _pump();
      first.emitPopup('https://b.example');
      await _pump();
      expect(controller.popups.value, hasLength(2));
      expect(controller.popups.value.first.popups.value, isEmpty);
      await controller.closePopup(controller.popups.value.last);
      expect(controller.popups.value, hasLength(1));
      await controller.dispose();
    });

    testWidgets('the popup layer covers the tab and its close button works', (
      tester,
    ) async {
      final controller = MorphWebViewController(initialUrl: 'https://e.com');
      await tester.pumpWidget(
        MaterialApp(home: MorphWebView(controller: controller)),
      );
      await tester.pump();
      await tester.runAsync(() async {
        await controller.attach(width: 800, height: 600, devicePixelRatio: 1);
        fake.lastSession!.emitPopup('https://login.example');
        await _pump();
      });
      await tester.pump();
      expect(find.byKey(const Key('morph-popup')), findsOneWidget);
      await tester.tap(find.byKey(const Key('morph-popup-close')));
      await tester.runAsync(_pump);
      await tester.pump();
      expect(find.byKey(const Key('morph-popup')), findsNothing);
      await tester.runAsync(controller.dispose);
    });
  });

  group('load errors', () {
    test('a failed load is kept until the next navigation', () async {
      final (controller, session) = await attached();
      const error = MorphLoadError(
        url: 'https://instgram.com/',
        kind: MorphLoadErrorKind.certificate,
      );
      session.emitLoadError(error);
      await _pump();
      expect(controller.loadError.value, same(error));
      await controller.reload();
      expect(controller.loadError.value, isNull);
      session.emitLoadError(error);
      await _pump();
      await controller.loadUrl('https://instagram.com/');
      expect(controller.loadError.value, isNull);
      await controller.dispose();
    });

    test('engine errors map to kinds', () {
      expect(
        morphLoadErrorKindFor(domain: 'WebKitNetworkError', code: 0, tls: true),
        MorphLoadErrorKind.certificate,
      );
      expect(
        morphLoadErrorKindFor(domain: 'g-resolver-error-quark', code: 0),
        MorphLoadErrorKind.notFound,
      );
      expect(
        morphLoadErrorKindFor(domain: 'g-io-error-quark', code: 24),
        MorphLoadErrorKind.timedOut,
      );
      expect(
        morphLoadErrorKindFor(domain: 'g-io-error-quark', code: 39),
        MorphLoadErrorKind.unreachable,
      );
      expect(
        morphLoadErrorKindFor(domain: 'WebKitNetworkError', code: 399),
        MorphLoadErrorKind.other,
      );
    });
  });

  group('hang grace', () {
    test('a short hang on a heavy page is never shown', () async {
      final (controller, session) = await attached(
        grace: const Duration(milliseconds: 80),
      );
      session.emitRenderer(MorphRendererFault.unresponsive);
      await _pump();
      expect(controller.rendererFault.value, isNull);
      session.emitRenderer(null);
      await Future<void>.delayed(const Duration(milliseconds: 120));
      expect(controller.rendererFault.value, isNull);
      await controller.dispose();
    });

    test('a hang that outlasts the grace is shown', () async {
      final (controller, session) = await attached(
        grace: const Duration(milliseconds: 40),
      );
      session.emitRenderer(MorphRendererFault.unresponsive);
      await Future<void>.delayed(const Duration(milliseconds: 80));
      expect(controller.rendererFault.value, MorphRendererFault.unresponsive);
      await controller.dispose();
    });

    test('a crash is shown at once, even during a hang grace', () async {
      final (controller, session) = await attached();
      session.emitRenderer(MorphRendererFault.unresponsive);
      session.emitRenderer(MorphRendererFault.crashed);
      await _pump();
      expect(controller.rendererFault.value, MorphRendererFault.crashed);
      await controller.dispose();
    });
  });
}
