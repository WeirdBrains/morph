import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph_webview/morph_webview.dart';

void main() {
  late FakeMorphWebViewPlatform fake;

  setUp(() {
    fake = FakeMorphWebViewPlatform();
    MorphWebViewPlatform.instance = fake;
  });

  testWidgets('controller loads a url and reports navigation', (tester) async {
    final controller = MorphWebViewController(
      initialUrl: 'https://example.com',
    );
    addTearDown(controller.dispose);

    await tester.pumpWidget(
      MaterialApp(
        home: SizedBox(
          width: 400,
          height: 300,
          child: MorphWebView(
            controller: controller,
            placeholder: const Text('placeholder'),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    expect(fake.loadedUrls, contains('https://example.com'));
    expect(controller.textureId, 1);
    expect(controller.renderPath.path, 'webkitgtk-snapshot');

    await controller.loadUrl('https://example.org/');
    await tester.pump();
    expect(controller.currentNavigation.url, 'https://example.org/');
    expect(controller.currentNavigation.canGoBack, isTrue);

    await controller.goBack();
    await tester.pump();
    expect(controller.currentNavigation.url, 'https://example.com');
  });
}
