import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/default_browser.dart';

class _Calls {
  final calls = <List<String>>[];
  final results = <String, ProcessResult>{};

  Future<ProcessResult> call(String exe, List<String> args) async {
    calls.add([exe, ...args]);
    return results['$exe ${args.join(' ')}'] ??
        results[exe] ??
        ProcessResult(0, 0, '', '');
  }
}

ProcessResult _ok(String out) => ProcessResult(0, 0, out, '');
ProcessResult _fail() => ProcessResult(0, 1, '', 'nope');

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('Linux', () {
    late Directory home;
    setUp(() => home = Directory.systemTemp.createTempSync('morph-xdg-'));
    tearDown(() => home.deleteSync(recursive: true));

    MorphLinuxDefaultBrowser linux(_Calls calls) => MorphLinuxDefaultBrowser(
      run: calls.call,
      executable: '/opt/morph/morph',
      environment: {'HOME': home.path, 'XDG_DATA_DIRS': '${home.path}/system'},
    );

    test('reads xdg-settings', () async {
      final calls = _Calls()
        ..results['xdg-settings get default-web-browser'] = _ok(
          'org.weirdbrains.morph.desktop\n',
        );
      expect(await linux(calls).status(), MorphDefaultBrowserState.isDefault);
      calls.results['xdg-settings get default-web-browser'] = _ok(
        'firefox.desktop\n',
      );
      expect(await linux(calls).status(), MorphDefaultBrowserState.notDefault);
      calls.results['xdg-settings get default-web-browser'] = _fail();
      expect(await linux(calls).status(), MorphDefaultBrowserState.unknown);
    });

    test('writes a user desktop entry and sets the default', () async {
      final calls = _Calls();
      final browser = linux(calls);
      calls.results['xdg-settings get default-web-browser'] = _ok(
        'org.weirdbrains.morph.desktop',
      );
      final made = browser.makeDefault();
      expect(await made, MorphDefaultBrowserRequest.madeDefault);
      final entry = File(browser.userEntryPath).readAsStringSync();
      expect(entry, contains('Exec=/opt/morph/morph %u'));
      expect(
        entry,
        contains('MimeType=x-scheme-handler/http;x-scheme-handler/https;'),
      );
      expect(
        browser.userEntryPath,
        '${home.path}/.local/share/applications/org.weirdbrains.morph.desktop',
      );
      expect(
        calls.calls,
        anyElement(
          equals([
            'xdg-settings',
            'set',
            'default-web-browser',
            'org.weirdbrains.morph.desktop',
          ]),
        ),
      );
    });

    test('leaves an installed or hand-written entry alone', () async {
      final system = File(
        '${home.path}/system/applications/org.weirdbrains.morph.desktop',
      )..createSync(recursive: true);
      system.writeAsStringSync('[Desktop Entry]\nExec=morph %u\n');
      final browser = linux(_Calls());
      expect(await browser.ensureDesktopEntry(), isNull);
      expect(File(browser.userEntryPath).existsSync(), isFalse);

      system.deleteSync();
      File(browser.userEntryPath)
        ..createSync(recursive: true)
        ..writeAsStringSync('[Desktop Entry]\nExec=my-morph %u\n');
      expect(await browser.ensureDesktopEntry(), isNull);
      expect(
        File(browser.userEntryPath).readAsStringSync(),
        contains('my-morph'),
      );
    });

    test('falls back to xdg-mime when xdg-settings cannot set', () async {
      final calls = _Calls()
        ..results['xdg-settings set default-web-browser '
                'org.weirdbrains.morph.desktop'] =
            _fail();
      expect(
        await linux(calls).makeDefault(),
        MorphDefaultBrowserRequest.failed, // get still says not default
      );
      expect(calls.calls.any((c) => c.first == 'xdg-mime'), isTrue);
    });
  });

  group('Windows', () {
    test('registers HKCU capabilities and opens Default apps', () async {
      final calls = _Calls();
      final browser = MorphWindowsDefaultBrowser(
        run: calls.call,
        executable: r'C:\Program Files\Morph\morph.exe',
      );
      expect(
        await browser.makeDefault(),
        MorphDefaultBrowserRequest.openedSettings,
      );
      final reg = calls.calls.where((c) => c.first == 'reg').toList();
      expect(reg.every((c) => c[2].startsWith(r'HKCU\')), isTrue);
      expect(
        reg.any(
          (c) =>
              c.contains(r'HKCU\Software\RegisteredApplications') &&
              c.contains('Morph'),
        ),
        isTrue,
      );
      expect(
        reg.any(
          (c) =>
              c[2].endsWith(r'Capabilities\URLAssociations') &&
              c.contains('https') &&
              c.contains('MorphURL'),
        ),
        isTrue,
      );
      expect(
        reg.any((c) => c.contains(r'"C:\Program Files\Morph\morph.exe" "%1"')),
        isTrue,
      );
      expect(calls.calls.last, [
        'explorer.exe',
        'ms-settings:defaultapps?registeredAppUser=Morph',
      ]);
    });

    test('reads the https UserChoice', () async {
      final calls = _Calls()
        ..results['reg'] = _ok('    ProgId    REG_SZ    MorphURL\n');
      final browser = MorphWindowsDefaultBrowser(run: calls.call);
      expect(await browser.status(), MorphDefaultBrowserState.isDefault);
      calls.results['reg'] = _ok('    ProgId    REG_SZ    ChromeHTML\n');
      expect(await browser.status(), MorphDefaultBrowserState.notDefault);
    });
  });

  group('Android and macOS channel', () {
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
