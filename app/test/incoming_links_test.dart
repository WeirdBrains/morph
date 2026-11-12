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
      return switch (call.method) {
        'initialLink' => 'https://from.intent/',
        'initialShare' => null,
        _ => throw StateError(call.method),
      };
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
  test('shared text: first link wins, bare hosts count, else a search', () {
    expect(
      MorphIncomingLinks.sharedTarget(
        'Look at this (https://news.example/a?b=1). Also http://two.example',
      ),
      'https://news.example/a?b=1',
    );
    expect(
      MorphIncomingLinks.sharedTarget('  https://only.example/x  '),
      'https://only.example/x',
    );
    expect(
      MorphIncomingLinks.sharedTarget('example.com/path'),
      'https://example.com/path',
    );
    expect(
      MorphIncomingLinks.sharedTarget('weird  brains\nmorph'),
      'https://duckduckgo.com/?q=weird+brains+morph',
    );
    expect(
      MorphIncomingLinks.sharedTarget('ftp://not.web/file'),
      startsWith('https://duckduckgo.com/?q='),
    );
    expect(
      Uri.parse(MorphIncomingLinks.searchUrl('a' * 5000)).queryParameters['q'],
      hasLength(2000),
    );
  });

  test('a launch share becomes a link or a search', () async {
    messenger.setMockMethodCallHandler(channel, (call) async {
      return switch (call.method) {
        'initialLink' => null,
        'initialShare' => 'what is weirdnet',
        _ => throw StateError(call.method),
      };
    });
    final links = MorphIncomingLinks(channel: channel);
    expect(await links.initialLinks(), [
      'https://duckduckgo.com/?q=what+is+weirdnet',
    ]);
  });

  test('shares while running stream in as links or searches', () async {
    final links = MorphIncomingLinks(channel: channel);
    final got = <String>[];
    final sub = links.later.listen(got.add);
    Future<void> push(Object? text) => messenger.handlePlatformMessage(
      channel.name,
      channel.codec.encodeMethodCall(MethodCall('share', text)),
      (_) {},
    );
    await push('Read https://shared.example/post thanks');
    await push('just words');
    await push('   ');
    await push(7);
    await Future<void>.delayed(Duration.zero);
    expect(got, [
      'https://shared.example/post',
      'https://duckduckgo.com/?q=just+words',
    ]);
    await sub.cancel();
    await links.dispose();
  });
}
