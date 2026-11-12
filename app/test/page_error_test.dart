import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/widgets/page_error.dart';
import 'package:morph_webview/morph_webview.dart';

void main() {
  test('an engine reason without a full stop still reads as a sentence', () {
    String body(String message) => morphLoadErrorText(
      MorphLoadError(
        url: 'https://instgram.com/',
        kind: MorphLoadErrorKind.certificate,
        message: message,
      ),
    ).$2;
    expect(body('Hostname mismatch'), contains('Hostname mismatch. Check'));
    expect(body('Expired.'), contains('Expired. Check'));
    expect(body(''), contains('not loaded. Check'));
  });

  test('a bad certificate names the site and the reason', () {
    final (title, body) = morphLoadErrorText(
      const MorphLoadError(
        url: 'https://instgram.com/',
        kind: MorphLoadErrorKind.certificate,
        message: 'The certificate is for a different site.',
      ),
    );
    expect(title, "This connection isn't private");
    expect(body, contains('instgram.com'));
    expect(body, contains('different site'));
  });

  testWidgets('a failed load shows Morph\'s page, and retry reloads', (
    tester,
  ) async {
    final fake = FakeMorphWebViewPlatform();
    MorphWebViewPlatform.instance = fake;
    final controller = MorphWebViewController(
      initialUrl: 'https://nope.invalid',
    );
    await tester.runAsync(
      () => controller.attach(width: 400, height: 300, devicePixelRatio: 1),
    );
    await tester.pumpWidget(
      MaterialApp(
        home: MorphLoadErrorGate(
          controller: controller,
          child: const SizedBox.expand(),
        ),
      ),
    );
    expect(find.byKey(const Key('page-error')), findsNothing);
    controller.loadError.value = const MorphLoadError(
      url: 'https://nope.invalid/',
      kind: MorphLoadErrorKind.notFound,
    );
    await tester.pump();
    expect(find.byKey(const Key('page-error')), findsOneWidget);
    expect(find.text("Can't find nope.invalid"), findsOneWidget);
    await tester.tap(find.byKey(const Key('page-error-retry')));
    await tester.pump();
    expect(find.byKey(const Key('page-error')), findsNothing);
    await tester.runAsync(controller.dispose);
  });
}
