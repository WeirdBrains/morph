import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/incoming_links.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('test/links');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test('only http(s) links with a host count', () {
    expect(MorphIncomingLinks.isWebLink('https://a.example/x?y=1'), isTrue);
    expect(MorphIncomingLinks.isWebLink('http://a.example'), isTrue);
    for (final bad in [
      '--debug',
      '/home/m/file.html',
      'file:///x',
      'https://',
      'javascript:alert(1)',
    ]) {
      expect(MorphIncomingLinks.isWebLink(bad), isFalse, reason: bad);
    }
  });

  test('launch links come from arguments and the platform', () async {
    messenger.setMockMethodCallHandler(channel, (call) async {
      expect(call.method, 'initialLink');
      return 'https://from.intent/';
    });
    final links = MorphIncomingLinks(
      arguments: ['--flag', 'https://from.args/'],
      channel: channel,
    );
    expect(await links.initialLinks(), [
      'https://from.args/',
      'https://from.intent/',
    ]);
  });

  test('no platform side still returns argument links', () async {
    final links = MorphIncomingLinks(
      arguments: ['https://a.example'],
      channel: channel,
    );
    expect(await links.initialLinks(), ['https://a.example']);
  });

  test('links opened later stream in; junk is dropped', () async {
    final links = MorphIncomingLinks(channel: channel);
    final got = <String>[];
    final sub = links.later.listen(got.add);
    Future<void> push(Object? url) => messenger.handlePlatformMessage(
      channel.name,
      channel.codec.encodeMethodCall(MethodCall('open', url)),
      (_) {},
    );
    await push('https://later.example/');
    await push('intent://nope');
    await push(42);
    await Future<void>.delayed(Duration.zero);
    expect(got, ['https://later.example/']);
    await sub.cancel();
    await links.dispose();
  });
}
