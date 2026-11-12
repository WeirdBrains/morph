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

  Future<(MorphWebViewController, FakeMorphWebViewSession)> attached() async {
    final controller = MorphWebViewController(
      initialUrl: 'about:blank',
      unresponsiveGrace: Duration.zero,
    );
    await controller.attach(width: 412, height: 800, devicePixelRatio: 2);
    return (controller, fake.lastSession!);
  }

  test('a crash marks only the tab whose renderer died', () async {
    final (a, sessionA) = await attached();
    final (b, _) = await attached();
    sessionA.emitRenderer(MorphRendererFault.crashed);
    await _pump();
    expect(a.rendererFault.value, MorphRendererFault.crashed);
    expect(b.rendererFault.value, isNull);
    await a.dispose();
    await b.dispose();
  });

  test('a crashed tab is not rebuilt behind the user\'s back', () async {
    final (controller, session) = await attached();
    session.emitRenderer(MorphRendererFault.crashed);
    await _pump();
    final verdict = await controller.ensureSurfaceHealthy();
    expect(verdict, MorphSurfaceVerdict.rendererGone);
    expect(session.surfaceChecks, 0);
    expect(session.surfaceRebuilds, 0);
    await controller.dispose();
  });

  test('recovering brings the page back in a new renderer', () async {
    final (controller, session) = await attached();
    session.emitRenderer(MorphRendererFault.killed);
    await _pump();
    await Future.wait([
      controller.recoverFromFault(),
      controller.recoverFromFault(),
    ]);
    expect(session.rendererRecoveries, 1);
    expect(session.rendererTerminations, 0);
    expect(controller.rendererFault.value, isNull);
    await controller.dispose();
  });

  test('a hung renderer is terminated before it is reloaded', () async {
    final (controller, session) = await attached();
    session.emitRenderer(MorphRendererFault.unresponsive);
    await _pump();
    expect(controller.rendererFault.value, MorphRendererFault.unresponsive);
    await controller.recoverFromFault();
    await _pump();
    expect(session.rendererTerminations, 1);
    expect(session.rendererRecoveries, 1);
    // The kill that reload asked for is not shown as a new crash.
    expect(controller.rendererFault.value, isNull);
    await controller.dispose();
  });

  test('a hang that clears by itself clears the tab', () async {
    final (controller, session) = await attached();
    session.emitRenderer(MorphRendererFault.unresponsive);
    await _pump();
    session.emitRenderer(null);
    await _pump();
    expect(controller.rendererFault.value, isNull);
    await controller.dispose();
  });

  test('a late hang report or recovery never hides a crash', () async {
    final (controller, session) = await attached();
    session.emitRenderer(MorphRendererFault.crashed);
    session.emitRenderer(MorphRendererFault.unresponsive);
    session.emitRenderer(null);
    await _pump();
    expect(controller.rendererFault.value, MorphRendererFault.crashed);
    await controller.dispose();
  });

  test('recovering a healthy tab does nothing', () async {
    final (controller, session) = await attached();
    await controller.recoverFromFault();
    expect(session.rendererRecoveries, 0);
    await controller.dispose();
  });

  test('a crash keeps the backend hint for the crash page', () async {
    final (controller, session) = await attached();
    session.emitRenderer(
      MorphRendererFault.crashed,
      detail: 'No GStreamer decoder for Opus.',
    );
    await _pump();
    expect(controller.rendererDetail, 'No GStreamer decoder for Opus.');
    await controller.dispose();
  });
}
