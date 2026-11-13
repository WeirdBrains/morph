import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/recovery_phrase.dart';
import 'package:morph/src/share.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/identity_onboarding.dart';

import 'support/fake_morph_api.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  String? clipboard;
  final calls = <MethodCall>[];

  setUp(() {
    clipboard = null;
    calls.clear();
    messenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') {
        clipboard = (call.arguments as Map)['text'] as String?;
      } else if (call.method == 'Clipboard.getData') {
        return {'text': clipboard};
      }
      return null;
    });
    for (final channel in [
      MorphShare.channel,
      MorphRecoveryPhrase.saveChannel,
    ]) {
      messenger.setMockMethodCallHandler(channel, (call) async {
        calls.add(call);
        return true;
      });
    }
  });

  tearDown(() {
    debugDefaultTargetPlatformOverride = null;
    for (final channel in [
      MorphShare.channel,
      MorphRecoveryPhrase.saveChannel,
    ]) {
      messenger.setMockMethodCallHandler(channel, null);
    }
  });

  test('file names are morph-<profile>-recovery-phrase.txt', () {
    expect(morphRecoveryFileName('Work'), 'morph-work-recovery-phrase.txt');
    expect(
      morphRecoveryFileName('My Bank / 2026!'),
      'morph-my-bank-2026-recovery-phrase.txt',
    );
    expect(morphRecoveryFileName('  '), 'morph-profile-recovery-phrase.txt');
    expect(morphBackupFileName('My Bank'), 'morph-my-bank-identity-backup.age');
  });

  test('the file text numbers all 24 words and repeats them on one line', () {
    final words = List.generate(24, (i) => 'w$i').join(' ');
    final text = morphRecoveryFileText(
      profileName: 'Work',
      userId: 'u1',
      words: words,
    );
    expect(text, contains('Profile: Work'));
    expect(text, contains(' 1. w0\n'));
    expect(text, contains('24. w23\n'));
    expect(text.trimRight().split('\n').last, words);
  });

  testWidgets('a copied phrase is cleared from the clipboard after a minute', (
    tester,
  ) async {
    await MorphRecoveryPhrase.copy('secret words');
    expect(clipboard, 'secret words');
    await tester.pump(const Duration(seconds: 59));
    expect(clipboard, 'secret words');
    await tester.pump(const Duration(seconds: 2));
    expect(clipboard, '');
  });

  testWidgets('the clipboard is left alone if something else was copied', (
    tester,
  ) async {
    await MorphRecoveryPhrase.copy('secret words');
    clipboard = 'a link';
    await tester.pump(const Duration(minutes: 2));
    expect(clipboard, 'a link');
  });

  test(
    'Android shares through the share sheet and saves through SAF',
    () async {
      debugDefaultTargetPlatformOverride = TargetPlatform.android;
      expect(await MorphRecoveryPhrase.share('text', subject: 's'), isTrue);
      expect(calls.single.method, 'shareText');
      expect(clipboard, isNull);
      calls.clear();
      expect(
        await MorphRecoveryPhrase.save(
          'morph-work-recovery-phrase.txt',
          'text',
        ),
        MorphPhraseSaveResult.saved,
      );
      expect(calls.single.method, 'saveText');
      expect(calls.single.arguments, {
        'fileName': 'morph-work-recovery-phrase.txt',
        'text': 'text',
        'mimeType': 'text/plain',
      });
      calls.clear();
      expect(
        await MorphRecoveryPhrase.save(
          morphBackupFileName('Work'),
          'sealed',
          mimeType: 'application/octet-stream',
        ),
        MorphPhraseSaveResult.saved,
      );
      expect(calls.single.arguments, {
        'fileName': 'morph-work-identity-backup.age',
        'text': 'sealed',
        'mimeType': 'application/octet-stream',
      });
    },
  );

  test('desktop share copies, and save writes where the dialog says', () async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    expect(await MorphRecoveryPhrase.share('text'), isFalse);
    expect(calls, isEmpty);
    expect(clipboard, 'text');

    final dir = Directory.systemTemp.createTempSync('morph-phrase-');
    addTearDown(() => dir.deleteSync(recursive: true));
    final previous = MorphRecoveryPhrase.pickSavePath;
    addTearDown(() => MorphRecoveryPhrase.pickSavePath = previous);
    String? suggested;
    MorphRecoveryPhrase.pickSavePath = (name) async {
      suggested = name;
      return '${dir.path}/$name';
    };
    expect(
      await MorphRecoveryPhrase.save('morph-a-recovery-phrase.txt', 'abc'),
      MorphPhraseSaveResult.saved,
    );
    expect(suggested, 'morph-a-recovery-phrase.txt');
    expect(
      File('${dir.path}/morph-a-recovery-phrase.txt').readAsStringSync(),
      'abc',
    );
    MorphRecoveryPhrase.pickSavePath = (_) async => null;
    expect(
      await MorphRecoveryPhrase.save('x.txt', 'abc'),
      MorphPhraseSaveResult.cancelled,
    );
  });

  testWidgets('the words screen offers copy, share and download', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    tester.view.physicalSize = const Size(412, 1600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final api = FakeMorphApi()..onboarded = false;
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: Material(
          child: MorphIdentityOnboarding(
            profileName: 'Work',
            onCreate: (pw) => api.createProfileIdentity('work', pw),
            onImport: (w, pw) => api.importProfileIdentity('work', w, pw),
            onDone: (_) {},
            onSkip: () {},
          ),
        ),
      ),
    );
    await tester.tap(find.byKey(const Key('onboarding-create')));
    await tester.pumpAndSettle();
    for (final key in ['onboarding-password', 'onboarding-confirm']) {
      await tester.enterText(find.byKey(Key(key)), 'long enough');
    }
    await tester.tap(find.byKey(const Key('onboarding-submit')));
    await tester.pumpAndSettle();
    final words = await api.profileRecoveryWords('work', 'long enough');

    await tester.tap(find.byKey(const Key('recovery-copy')));
    await tester.pump();
    expect(clipboard, words);
    expect(find.textContaining('clipboard clears in 60'), findsOneWidget);

    await tester.tap(find.byKey(const Key('recovery-share')));
    await tester.pump();
    expect(calls.last.method, 'shareText');
    expect((calls.last.arguments as Map)['text'], contains(words));

    await tester.tap(find.byKey(const Key('recovery-save')));
    await tester.pump();
    expect(calls.last.method, 'saveText');
    expect(
      (calls.last.arguments as Map)['fileName'],
      'morph-work-recovery-phrase.txt',
    );
    expect(
      find.text('Saved morph-work-recovery-phrase.txt. Keep it offline.'),
      findsOneWidget,
    );
    // Let the clipboard clear timer run out so the test ends clean.
    await tester.pump(MorphRecoveryPhrase.clipboardLifetime);
    expect(clipboard, '');
    debugDefaultTargetPlatformOverride = null;
  });
}
