import 'package:flutter_test/flutter_test.dart';
import 'package:morph_webview/morph_webview.dart';
import 'package:morph_webview_inapp/src/renderer_watch.dart';

void main() {
  late InAppRendererWatch watch;
  late List<MorphRendererFault?> seen;

  setUp(() {
    watch = InAppRendererWatch(terminateWait: const Duration(seconds: 1));
    seen = [];
    watch.events.listen((e) => seen.add(e.fault));
  });

  tearDown(() => watch.close());

  Future<void> pump() => Future<void>.delayed(Duration.zero);

  test('a crash and a system kill are told apart', () async {
    watch.renderProcessGone(didCrash: true);
    await pump();
    expect(seen, [MorphRendererFault.crashed]);
    expect(watch.gone, isTrue);
    watch.rebuilt();
    expect(watch.gone, isFalse);
    watch.renderProcessGone(didCrash: false);
    await pump();
    expect(seen.last, MorphRendererFault.killed);
  });

  test('a macOS web content termination is a crash', () async {
    watch.webContentProcessTerminated();
    await pump();
    expect(seen, [MorphRendererFault.crashed]);
  });

  test('a hang is reported, and clears when it answers again', () async {
    expect(watch.renderProcessUnresponsive(), isFalse);
    watch.renderProcessResponsive();
    await pump();
    expect(seen, [MorphRendererFault.unresponsive, null]);
  });

  test('terminate ends the hung renderer at its next report', () async {
    watch.renderProcessUnresponsive();
    final terminating = watch.terminate();
    // Android asks again while still hung; now the answer is terminate.
    expect(watch.renderProcessUnresponsive(), isTrue);
    watch.renderProcessGone(didCrash: false);
    await terminating;
    expect(watch.gone, isTrue);
    // Once gone, later reports do not ask to terminate again.
    expect(watch.renderProcessUnresponsive(), isFalse);
  });

  test('terminate gives up after its wait and does not stick', () async {
    final started = DateTime.now();
    await watch.terminate();
    expect(DateTime.now().difference(started).inMilliseconds, greaterThan(900));
    expect(watch.renderProcessUnresponsive(), isFalse);
  });

  test('terminating a gone renderer returns at once', () async {
    watch.renderProcessGone(didCrash: true);
    await watch.terminate();
    expect(watch.renderProcessUnresponsive(), isFalse);
  });
}
