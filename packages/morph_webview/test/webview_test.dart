import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
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
    expect(controller.handlesPageBridge, isFalse);

    await controller.loadUrl('https://example.org/');
    await tester.pump();
    expect(controller.currentNavigation.url, 'https://example.org/');
    expect(controller.currentNavigation.canGoBack, isTrue);

    await controller.goBack();
    await tester.pump();
    expect(controller.currentNavigation.url, 'https://example.com');
  });

  testWidgets('embeds a platform view when the session provides one', (
    tester,
  ) async {
    fake = FakeMorphWebViewPlatform(
      renderPath: const MorphRenderPathInfo(path: 'wkwebview', reason: 'fake'),
      platformView: const Text('platform-view'),
    );
    MorphWebViewPlatform.instance = fake;
    final controller = MorphWebViewController(
      initialUrl: 'https://example.com',
    );
    addTearDown(controller.dispose);

    await tester.pumpWidget(
      MaterialApp(
        home: SizedBox(
          width: 400,
          height: 300,
          child: MorphWebView(controller: controller),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    expect(find.text('platform-view'), findsOneWidget);
    expect(controller.textureId, isNull);
    expect(controller.renderPath.path, 'wkwebview');
  });

  testWidgets('forwards hover, leave, click, scroll and cursor shape', (
    tester,
  ) async {
    final controller = MorphWebViewController(
      initialUrl: 'https://example.com',
    );
    addTearDown(controller.dispose);

    await tester.pumpWidget(
      MaterialApp(
        home: SizedBox(
          width: 400,
          height: 300,
          child: MorphWebView(controller: controller),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    final session = fake.lastSession!;
    final surface = find.byKey(const Key('morph-webview-surface'));
    expect(surface, findsOneWidget);
    final center = tester.getCenter(surface);

    final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await gesture.addPointer(location: Offset.zero);
    addTearDown(gesture.removePointer);
    await tester.pump();

    await gesture.moveTo(center);
    await tester.pump();
    await tester.pump();

    final phases = session.pointers.map((e) => e.phase).toList();
    expect(phases, contains('enter'));
    expect(phases, contains('hover'));
    final hover = session.pointers.lastWhere((e) => e.phase == 'hover');
    expect(hover.x, closeTo(center.dx, 1));
    expect(hover.y, closeTo(center.dy, 1));
    expect(hover.buttons, 0);
    expect(hover.button, 0);

    session.emitCursor('pointer');
    await tester.pump();
    expect(controller.cursorShape.value, 'pointer');
    expect(
      morphCursorForShape(controller.cursorShape.value),
      SystemMouseCursors.click,
    );

    await tester.sendEventToBinding(
      PointerScrollEvent(
        position: tester.getCenter(surface),
        scrollDelta: const Offset(0, 80),
      ),
    );
    await tester.pump();
    final scroll = session.pointers.lastWhere((e) => e.phase == 'scroll');
    expect(scroll.scrollDeltaY, 80);

    await tester.tapAt(center);
    await tester.pump();
    expect(session.pointers.where((e) => e.phase == 'down'), isNotEmpty);
    expect(session.pointers.where((e) => e.phase == 'up'), isNotEmpty);
    expect(controller.focusNode.hasFocus, isTrue);

    await gesture.moveTo(const Offset(2000, 2000));
    await tester.pump();
    await tester.pump();
    expect(session.pointers.where((e) => e.phase == 'leave'), isNotEmpty);
  });

  testWidgets('forwards key and text events once the view has focus', (
    tester,
  ) async {
    final controller = MorphWebViewController(
      initialUrl: 'https://example.com',
    );
    addTearDown(controller.dispose);

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Column(
            children: [
              const SizedBox(height: 40, child: TextField(key: Key('url-bar'))),
              SizedBox(
                width: 400,
                height: 300,
                child: MorphWebView(controller: controller),
              ),
            ],
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    final session = fake.lastSession!;

    // Address bar has a text field; keys typed there must not reach the view.
    await tester.tap(find.byKey(const Key('url-bar')));
    await tester.pump();
    await tester.enterText(find.byKey(const Key('url-bar')), 'hello');
    expect(session.keys, isEmpty);

    controller.requestFocus();
    await tester.pump();
    expect(controller.focusNode.hasFocus, isTrue);

    await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.sendKeyEvent(LogicalKeyboardKey.backspace);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyV);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();

    final labels = session.keys.map((e) => '${e.phase}:${e.key}').toList();
    expect(labels, contains('down:A'));
    expect(labels, contains('up:A'));
    expect(labels, contains('down:Enter'));
    expect(labels, contains('down:Backspace'));
    expect(labels.where((e) => e.endsWith(':V')), isNotEmpty);
    final aDown = session.keys.firstWhere(
      (e) => e.phase == 'down' && e.key == 'A',
    );
    expect(aDown.text.toLowerCase(), 'a');
    final vDown = session.keys.firstWhere(
      (e) => e.phase == 'down' && e.key == 'V',
    );
    expect(vDown.ctrl, isTrue);

    expect(session.imes.where((e) => e.phase == 'focus'), isNotEmpty);
  });
}
