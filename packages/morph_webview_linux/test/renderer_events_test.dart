import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph_webview/morph_webview.dart';
import 'package:morph_webview_linux/morph_webview_linux.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('morph_webview_linux_test');
  final calls = <MethodCall>[];

  setUp(() {
    calls.clear();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          return null;
        });
  });

  test('fault names map to renderer faults', () {
    expect(morphRendererFaultFromName('crashed'), MorphRendererFault.crashed);
    expect(
      morphRendererFaultFromName('outOfMemory'),
      MorphRendererFault.outOfMemory,
    );
    expect(morphRendererFaultFromName('killed'), MorphRendererFault.killed);
    expect(
      morphRendererFaultFromName('unresponsive'),
      MorphRendererFault.unresponsive,
    );
    expect(morphRendererFaultFromName(null), isNull);
    expect(morphRendererFaultFromName('new'), MorphRendererFault.crashed);
  });

  test('a session only sees renderer events for its own view', () async {
    final events = StreamController<Map<Object?, Object?>>.broadcast();
    final one = MorphWebViewLinux.sessionForTesting(
      channel: channel,
      viewId: 1,
      events: events.stream,
    );
    final two = MorphWebViewLinux.sessionForTesting(
      channel: channel,
      viewId: 2,
      events: events.stream,
    );
    final seenOne = <MorphRendererFault?>[];
    final seenTwo = <MorphRendererFault?>[];
    one.renderer.listen((e) => seenOne.add(e.fault));
    two.renderer.listen((e) => seenTwo.add(e.fault));
    events.add({'viewId': 2, 'type': 'renderer', 'fault': 'crashed'});
    events.add({'viewId': 1, 'type': 'new-window', 'url': 'x'});
    events.add({'viewId': 1, 'type': 'renderer', 'fault': 'unresponsive'});
    events.add({'viewId': 1, 'type': 'renderer', 'fault': null});
    await Future<void>.delayed(Duration.zero);
    expect(seenOne, [MorphRendererFault.unresponsive, null]);
    expect(seenTwo, [MorphRendererFault.crashed]);
    await one.dispose();
    await two.dispose();
    await events.close();
  });

  test('terminating a hung renderer targets its own view', () async {
    final session = MorphWebViewLinux.sessionForTesting(
      channel: channel,
      viewId: 7,
      events: const Stream.empty(),
    );
    await session.terminateRenderer();
    expect(calls.single.method, 'terminateWebProcess');
    expect(calls.single.arguments, {'viewId': 7});
    await session.dispose();
  });

  test('a TLS load error maps to a certificate error page', () {
    final error = morphLoadErrorFromEvent({
      'type': 'load-error',
      'url': 'https://instgram.com/',
      'domain': 'g-tls-error-quark',
      'code': 3,
      'message': 'The certificate is for a different site.',
      'tls': true,
    });
    expect(error.kind, MorphLoadErrorKind.certificate);
    expect(error.url, 'https://instgram.com/');
  });

  test(
    'a session surfaces its load errors, popups and close requests',
    () async {
      final events = StreamController<Map<Object?, Object?>>.broadcast();
      final tab = MorphWebViewLinux.sessionForTesting(
        channel: channel,
        viewId: 1,
        events: events.stream,
      );
      final errors = <MorphLoadError>[];
      final popups = <MorphWebViewSession>[];
      tab.loadErrors.listen(errors.add);
      tab.popups.listen(popups.add);
      events.add({
        'viewId': 1,
        'type': 'load-error',
        'url': 'https://nope.invalid/',
        'domain': 'g-resolver-error-quark',
        'code': 1,
        'tls': false,
      });
      // Another tab's popup is not ours.
      events.add({'viewId': 9, 'type': 'new-window', 'openerViewId': 2});
      events.add({
        'viewId': 5,
        'type': 'new-window',
        'openerViewId': 1,
        'textureId': 55,
        'url': 'https://accounts.example/',
      });
      await Future<void>.delayed(Duration.zero);
      expect(errors.single.kind, MorphLoadErrorKind.notFound);
      expect(popups, hasLength(1));
      final popup = popups.single;
      expect(popup.textureId, 55);
      var closed = 0;
      popup.closeRequested.listen((_) => closed++);
      events.add({'viewId': 5, 'type': 'closed'});
      await Future<void>.delayed(Duration.zero);
      expect(closed, 1);
      calls.clear();
      await popup.dispose();
      // The native view is already gone after window.close().
      expect(calls.where((c) => c.method == 'dispose'), isEmpty);
      await tab.dispose();
      await events.close();
    },
  );
}
