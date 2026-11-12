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
}
