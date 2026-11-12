import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph_webview_inapp/src/profile_store.dart';

void main() {
  test('profile storage paths follow profiles/<id>/<leaf>', () {
    expect(
      morphProfileStorageDir(
        profileId: 'default',
        leaf: 'webview2',
        morphHome: r'C:\morph-home',
        pathSeparator: r'\',
      ),
      r'C:\morph-home\profiles\default\webview2',
    );
    expect(
      morphProfileStorageDir(
        profileId: 'p1',
        leaf: 'webview2',
        morphHome: '/tmp/h',
        pathSeparator: '/',
      ),
      '/tmp/h/profiles/p1/webview2',
    );
  });

  test('falls back to user home /.morph when MORPH_HOME is empty', () {
    expect(
      morphProfileStorageDir(
        profileId: 'default',
        leaf: 'webview2',
        morphHome: '',
        userHome: '/Users/mike',
        pathSeparator: '/',
      ),
      '/Users/mike/.morph/profiles/default/webview2',
    );
  });

  test('two profile ids never share a directory', () {
    final a = morphProfileStorageDir(
      profileId: 'default',
      leaf: 'webview2',
      morphHome: '/tmp/h',
      pathSeparator: '/',
    );
    final b = morphProfileStorageDir(
      profileId: 'p1',
      leaf: 'webview2',
      morphHome: '/tmp/h',
      pathSeparator: '/',
    );
    final c = morphProfileStorageDir(
      profileId: 'p2',
      leaf: 'webview2',
      morphHome: '/tmp/h',
      pathSeparator: '/',
    );
    expect({a, b, c}.length, 3);
  });

  group('android profiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    const channel = MethodChannel('test/morph-profile');
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

    tearDown(() => messenger.setMockMethodCallHandler(channel, null));

    test('default keeps the WebView default store; others get their own', () {
      expect(morphAndroidProfileName('default'), 'Default');
      expect(morphAndroidProfileName(''), 'Default');
      expect(morphAndroidProfileName('p1'), 'morph-p1');
      final names = {
        for (final id in ['default', 'p1', 'p2', 'work'])
          morphAndroidProfileName(id),
      };
      expect(names.length, 4);
    });

    test('prepare sends the profile and keep-alive id', () async {
      final calls = <MethodCall>[];
      messenger.setMockMethodCallHandler(channel, (call) async {
        calls.add(call);
        return {'isolated': true, 'profile': 'morph-p1'};
      });
      final result = await prepareAndroidVia(channel, 'p1', 'morph-view-3-9');
      expect(result.isolated, isTrue);
      expect(result.name, 'morph-p1');
      expect(calls.single.method, 'prepareAndroid');
      expect(calls.single.arguments, {
        'profileId': 'p1',
        'viewKey': 'morph-view-3-9',
      });
    });

    test('an old WebView reports the profile as not isolated', () async {
      messenger.setMockMethodCallHandler(
        channel,
        (call) async => {'isolated': false, 'profile': 'Default'},
      );
      final result = await prepareAndroidVia(channel, 'p1', 'k');
      expect(result.isolated, isFalse);
    });

    test(
      'without the native hook only the default profile is isolated',
      () async {
        expect(
          (await prepareAndroidVia(channel, 'default', 'k')).isolated,
          isTrue,
        );
        expect((await prepareAndroidVia(channel, 'p1', 'k')).isolated, isFalse);
      },
    );

    test('stale sweep only touches deleted Morph profiles', () {
      expect(
        staleAndroidProfiles(
          ['Default', 'morph-p1', 'morph-p2', 'someone-else'],
          ['default', 'p1'],
        ),
        ['p2'],
      );
      expect(staleAndroidProfiles(['Default'], ['default']), isEmpty);
    });
  });
}
