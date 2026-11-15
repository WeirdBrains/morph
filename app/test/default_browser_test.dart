import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/default_browser.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('Android channel', () {
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    tearDown(
      () =>
          messenger.setMockMethodCallHandler(MorphDefaultBrowser.channel, null),
    );

    test('maps status and request answers', () async {
      final calls = <String>[];
      messenger.setMockMethodCallHandler(MorphDefaultBrowser.channel, (
        call,
      ) async {
        calls.add(call.method);
        return call.method == 'status' ? true : 'openedSettings';
      });
      const browser = MorphChannelDefaultBrowser();
      expect(await browser.status(), MorphDefaultBrowserState.isDefault);
      expect(
        await browser.makeDefault(),
        MorphDefaultBrowserRequest.openedSettings,
      );
      expect(calls, ['status', 'request']);
    });

    test('a missing platform side is unknown, not a crash', () async {
      const browser = MorphChannelDefaultBrowser();
      expect(await browser.status(), MorphDefaultBrowserState.unknown);
      expect(await browser.makeDefault(), MorphDefaultBrowserRequest.failed);
    });
  });
}
