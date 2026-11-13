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

    test('rounding noise does not count as content', () {
      final frame = _flat(100 * 100, [250, 250, 250, 255]);
      frame[401] = 252;
      expect(morphFrameIsFlat(frame), isTrue);
    });

    test('a few pixels of small text on white are content', () {
      // A bare "403 Forbidden" in the corner of a 270 px wide capture.
      final frame = _flat(270 * 600, [255, 255, 255, 255]);
      for (final i in [5000, 5001, 5004, 5270, 5275, 5540]) {
        frame.setRange(i * 4, i * 4 + 3, [120, 120, 120]);
      }
      expect(morphFrameIsFlat(frame), isFalse);
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

    test('a check waits until the page has stopped scrolling', () async {
      final controller = MorphWebViewController(
        initialUrl: 'about:blank',
        surfaceQuiet: const Duration(milliseconds: 150),
      );
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      session.lastUserScroll = DateTime.now();
      final watch = Stopwatch()..start();
      final verdict = controller.ensureSurfaceHealthy();
      await Future<void>.delayed(const Duration(milliseconds: 50));
      expect(session.surfaceChecks, 0, reason: 'no capture mid-fling');
      expect(session.surfaceRepaints, 0);
      expect(await verdict, MorphSurfaceVerdict.healthy);
      expect(watch.elapsedMilliseconds, greaterThanOrEqualTo(120));
      expect(session.surfaceChecks, 1);
      await controller.dispose();
    });

    test('a page that keeps scrolling is never captured', () async {
      final controller = MorphWebViewController(
        initialUrl: 'about:blank',
        surfaceQuiet: const Duration(milliseconds: 100),
        surfaceQuietMax: const Duration(milliseconds: 300),
      );
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      final scrolling = Timer.periodic(
        const Duration(milliseconds: 20),
        (_) => session.lastUserScroll = DateTime.now(),
      );
      session.lastUserScroll = DateTime.now();
      try {
        expect(
          await controller.ensureSurfaceHealthy(),
          MorphSurfaceVerdict.unknown,
        );
      } finally {
        scrolling.cancel();
      }
      expect(session.surfaceChecks, 0);
      expect(session.surfaceRebuilds, 0);
      await controller.dispose();
    });

    test('the confirming look also waits for scrolling to stop', () async {
      final controller = MorphWebViewController(
        initialUrl: 'about:blank',
        surfaceSettle: Duration.zero,
        surfaceQuiet: const Duration(milliseconds: 150),
      );
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      session.surfaceVerdicts.addAll([
        MorphSurfaceVerdict.blank,
        MorphSurfaceVerdict.healthy,
      ]);
      final gate = Completer<void>();
      session.checkGate = gate;
      final verdict = controller.ensureSurfaceHealthy();
      await Future<void>.delayed(Duration.zero);
      expect(session.surfaceChecks, 1);
      // The user starts a fling while the first look is being taken.
      session.lastUserScroll = DateTime.now();
      session.checkGate = null;
      gate.complete();
      await Future<void>.delayed(const Duration(milliseconds: 50));
      expect(session.surfaceChecks, 1, reason: 'confirm waits for the fling');
      expect(await verdict, MorphSurfaceVerdict.healthy);
      expect(session.surfaceChecks, 2);
      expect(session.surfaceRebuilds, 0);
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

    test('a blank surface is confirmed, rebuilt and checked again', () async {
      final controller = MorphWebViewController(
        initialUrl: 'about:blank',
        surfaceSettle: Duration.zero,
      );
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      session.surfaceVerdicts.addAll([
        MorphSurfaceVerdict.blank,
        MorphSurfaceVerdict.blank,
        MorphSurfaceVerdict.healthy,
      ]);
      expect(
        await controller.ensureSurfaceHealthy(),
        MorphSurfaceVerdict.healthy,
      );
      expect(session.surfaceRebuilds, 1);
      expect(session.surfaceRepaints, 2);
      expect(session.surfaceChecks, 3);
      expect(controller.surfaceRebuilds, 1);
      await controller.dispose();
    });

    test('a surface that paints after a second nudge is not rebuilt', () async {
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
      expect(session.surfaceRebuilds, 0);
      expect(session.surfaceRepaints, 2);
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

    test('heals asked for during a check share one follow-up', () async {
      final controller = MorphWebViewController(initialUrl: 'about:blank');
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      final gate = Completer<void>();
      session.checkGate = gate;
      final first = controller.ensureSurfaceHealthy();
      await Future<void>.delayed(Duration.zero);
      expect(session.surfaceChecks, 1);
      final later = [
        controller.ensureSurfaceHealthy(),
        controller.ensureSurfaceHealthy(),
      ];
      session.checkGate = null;
      gate.complete();
      await Future.wait([first, ...later]);
      // The running check plus exactly one follow-up for the late callers.
      expect(session.surfaceChecks, 2);
      await controller.dispose();
    });

    test('a check asked for mid-load still sees the finished page', () async {
      // The bookmark white page: closing the library sheet starts a check
      // while the bookmark is still loading (no content yet), and the
      // finished load's check used to join it and never look again.
      final controller = MorphWebViewController(initialUrl: 'about:blank');
      await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
      final session = fake.lastSession!;
      session.surfaceVerdicts.addAll([
        MorphSurfaceVerdict.unknown,
        MorphSurfaceVerdict.blank,
      ]);
      final gate = Completer<void>();
      session.checkGate = gate;
      final sheetClosed = controller.ensureSurfaceHealthy();
      await Future<void>.delayed(Duration.zero);
      final loadFinished = controller.ensureSurfaceHealthy();
      session.checkGate = null;
      gate.complete();
      expect(await sheetClosed, MorphSurfaceVerdict.unknown);
      await loadFinished;
      expect(session.surfaceRebuilds, 1);
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
