import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph_webview/morph_webview.dart';

Uint8List _flat(int pixels, List<int> rgba) {
  final out = Uint8List(pixels * 4);
  for (var i = 0; i < pixels; i++) {
    out.setRange(i * 4, i * 4 + 4, rgba);
  }
  return out;
}

const _content = MorphSurfaceProbe(
  readyState: 'complete',
  textLength: 40,
  mediaCount: 0,
  width: 412,
  height: 800,
);

void main() {
  group('frame classifier', () {
    test('a flat white frame is flat', () {
      expect(morphFrameIsFlat(_flat(64 * 64, [255, 255, 255, 255])), isTrue);
    });

    test('compression noise and a caret do not count as content', () {
      final frame = _flat(100 * 100, [250, 250, 250, 255]);
      frame[0] = 253; // noise on the reference pixel's neighbour channel
      frame[400] = 0; // one dark pixel (caret)
      expect(morphFrameIsFlat(frame), isTrue);
    });

    test('text on white is not flat', () {
      final frame = _flat(100 * 100, [255, 255, 255, 255]);
      for (var i = 0; i < 300; i++) {
        frame.setRange(i * 4 * 7, i * 4 * 7 + 3, [20, 20, 20]);
      }
      expect(morphFrameIsFlat(frame), isFalse);
    });
  });

  group('probe', () {
    test('parses Android raw JSON and JSON-quoted results', () {
      const raw =
          '{"readyState":"complete","text":12,"media":1,"w":412,"h":800}';
      for (final value in [raw, '"${raw.replaceAll('"', r'\"')}"']) {
        final probe = MorphSurfaceProbe.tryParse(value)!;
        expect(probe.readyState, 'complete');
        expect(probe.textLength, 12);
        expect(probe.mediaCount, 1);
        expect(probe.hasVisibleContent, isTrue);
      }
    });

    test('rejects garbage', () {
      expect(MorphSurfaceProbe.tryParse(null), isNull);
      expect(MorphSurfaceProbe.tryParse('not json'), isNull);
      expect(MorphSurfaceProbe.tryParse('[1,2]'), isNull);
    });

    test('an empty or still-loading page is never judged blank', () {
      const empty = MorphSurfaceProbe(
        readyState: 'complete',
        textLength: 0,
        mediaCount: 0,
        width: 412,
        height: 800,
      );
      const loading = MorphSurfaceProbe(
        readyState: 'loading',
        textLength: 30,
        mediaCount: 0,
        width: 412,
        height: 800,
      );
      for (final probe in [empty, loading]) {
        expect(
          morphClassifySurface(probe: probe, frameIsFlat: true),
          MorphSurfaceVerdict.unknown,
        );
      }
    });

    test('content behind a flat frame is a blank surface', () {
      expect(
        morphClassifySurface(probe: _content, frameIsFlat: true),
        MorphSurfaceVerdict.blank,
      );
      expect(
        morphClassifySurface(probe: _content, frameIsFlat: false),
        MorphSurfaceVerdict.healthy,
      );
      expect(
        morphClassifySurface(probe: _content, frameIsFlat: null),
        MorphSurfaceVerdict.unknown,
      );
    });
  });

  group('controller', () {
    late FakeMorphWebViewPlatform fake;

    setUp(() {
      fake = FakeMorphWebViewPlatform();
      MorphWebViewPlatform.instance = fake;
    });

    test('concurrent attaches create one native view', () async {
      final gate = Completer<void>();
      fake.createGate = gate;
      final controller = MorphWebViewController(initialUrl: 'about:blank');
      // Keyboard and sheet layouts ask again while the first create runs.
      final first = controller.attach(
        width: 412,
        height: 800,
        devicePixelRatio: 2,
      );
      final second = controller.attach(
        width: 412,
        height: 420,
        devicePixelRatio: 2,
      );
      final third = controller.attach(
        width: 412,
        height: 800,
        devicePixelRatio: 2,
      );
      gate.complete();
      await Future.wait([first, second, third]);
      expect(fake.sessions, hasLength(1));
      expect(controller.isAttached, isTrue);
      await controller.dispose();
    });

    test('an address submitted mid-create is loaded, not dropped', () async {
      final gate = Completer<void>();
      fake.createGate = gate;
      final controller = MorphWebViewController(initialUrl: 'about:blank');
      final attaching = controller.attach(
        width: 412,
        height: 800,
        devicePixelRatio: 2,
      );
      await controller.loadUrl('https://www.linkedin.com/');
      gate.complete();
      await attaching;
      await Future<void>.delayed(Duration.zero);
      expect(
        fake.sessions.single.history,
        contains('https://www.linkedin.com/'),
      );
      expect(controller.currentNavigation.url, 'https://www.linkedin.com/');
      await controller.dispose();
    });

    test('a healthy surface only gets a repaint nudge', () async {
      final controller = MorphWebViewController(initialUrl: 'about:blank');
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      expect(
        await controller.ensureSurfaceHealthy(),
        MorphSurfaceVerdict.healthy,
      );
      expect(session.surfaceRepaints, 1);
      expect(session.surfaceRebuilds, 0);
      await controller.dispose();
    });

    test('a blank surface is rebuilt and checked again', () async {
      final controller = MorphWebViewController(
        initialUrl: 'about:blank',
        surfaceSettle: Duration.zero,
      );
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      session.surfaceVerdicts.addAll([
        MorphSurfaceVerdict.blank,
        MorphSurfaceVerdict.healthy,
      ]);
      expect(
        await controller.ensureSurfaceHealthy(),
        MorphSurfaceVerdict.healthy,
      );
      expect(session.surfaceRebuilds, 1);
      expect(session.surfaceChecks, 2);
      expect(controller.surfaceRebuilds, 1);
      await controller.dispose();
    });

    test('rebuilds are rate limited so a flat page cannot loop', () async {
      var now = DateTime(2026, 11, 12, 8);
      final controller = MorphWebViewController(
        initialUrl: 'about:blank',
        surfaceSettle: Duration.zero,
        clock: () => now,
      );
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      session.surfaceVerdicts.add(MorphSurfaceVerdict.blank);
      await controller.ensureSurfaceHealthy();
      await controller.ensureSurfaceHealthy();
      await controller.ensureSurfaceHealthy();
      expect(session.surfaceRebuilds, 1);
      now = now.add(const Duration(seconds: 21));
      await controller.ensureSurfaceHealthy();
      expect(session.surfaceRebuilds, 2);
      await controller.dispose();
    });

    test('concurrent heals share one check', () async {
      final controller = MorphWebViewController(initialUrl: 'about:blank');
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      await Future.wait([
        controller.ensureSurfaceHealthy(),
        controller.ensureSurfaceHealthy(),
        controller.ensureSurfaceHealthy(),
      ]);
      expect(session.surfaceChecks, 1);
      await controller.dispose();
    });

    test('a lost renderer rebuilds the view on its own', () async {
      final controller = MorphWebViewController(initialUrl: 'about:blank');
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      session.emitSurfaceLost();
      await Future<void>.delayed(Duration.zero);
      await Future<void>.delayed(Duration.zero);
      expect(session.surfaceRebuilds, 1);
      expect(controller.surfaceRebuilds, 1);
      await controller.dispose();
    });
  });

  testWidgets('keyboard-driven relayouts during create make one view', (
    tester,
  ) async {
    final fake = FakeMorphWebViewPlatform();
    MorphWebViewPlatform.instance = fake;
    final gate = Completer<void>();
    fake.createGate = gate;
    final controller = MorphWebViewController(initialUrl: 'about:blank');
    addTearDown(controller.dispose);
    final height = ValueNotifier<double>(800);
    await tester.pumpWidget(
      MaterialApp(
        home: ValueListenableBuilder<double>(
          valueListenable: height,
          builder: (context, h, _) => Align(
            alignment: Alignment.topLeft,
            child: SizedBox(
              width: 400,
              height: h,
              child: MorphWebView(controller: controller),
            ),
          ),
        ),
      ),
    );
    // The address sheet's keyboard slides in and out while create runs.
    for (final h in [700.0, 520.0, 420.0, 520.0, 800.0]) {
      height.value = h;
      await tester.pump();
      await tester.pump();
    }
    gate.complete();
    await tester.pump();
    await tester.pump();
    await tester.pump();
    expect(fake.sessions, hasLength(1));
    expect(controller.isAttached, isTrue);
  });
}
