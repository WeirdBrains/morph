import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/share.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  String? clipboard;

  setUp(() {
    clipboard = null;
    messenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') {
        clipboard = (call.arguments as Map)['text'] as String?;
      }
      return null;
    });
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(MorphShare.channel, null);
    messenger.setMockMethodCallHandler(SystemChannels.platform, null);
    debugDefaultTargetPlatformOverride = null;
  });

  test('Android opens the share sheet with the link and title', () async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    MethodCall? sent;
    messenger.setMockMethodCallHandler(MorphShare.channel, (call) async {
      sent = call;
      return true;
    });
    expect(await MorphShare.shareLink('https://a.test/', title: 'A'), isTrue);
    expect(sent!.method, 'shareText');
    expect(sent!.arguments, {'text': 'https://a.test/', 'subject': 'A'});
    expect(clipboard, isNull);
  });

  test('Android falls back to the clipboard when no app can share', () async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    messenger.setMockMethodCallHandler(
      MorphShare.channel,
      (call) async => false,
    );
    expect(await MorphShare.shareLink('https://b.test/'), isFalse);
    expect(clipboard, 'https://b.test/');
  });

  test('desktop copies the link', () async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    var called = false;
    messenger.setMockMethodCallHandler(MorphShare.channel, (call) async {
      called = true;
      return true;
    });
    expect(await MorphShare.shareLink('https://c.test/'), isFalse);
    expect(called, isFalse);
    expect(clipboard, 'https://c.test/');
  });
}
