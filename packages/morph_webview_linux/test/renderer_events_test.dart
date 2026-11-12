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
}
