import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/identity_files.dart';
import 'package:morph/src/recovery_phrase.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/identity_backup.dart';
import 'package:morph/src/widgets/identity_onboarding.dart';

import 'support/fake_morph_api.dart';

Future<void> _app(WidgetTester tester, Widget home) async {
  tester.view.physicalSize = const Size(600, 1400);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(theme: MorphTheme.dark, home: home));
}

/// Every password field under [of] has its own eye toggle.
void _expectEyes(WidgetTester tester, List<String> keys) {
  for (final key in keys) {
    expect(
      find.descendant(
        of: find.byKey(Key(key)),
        matching: find.byKey(const Key('password-visibility')),
      ),
      findsOneWidget,
      reason: key,
    );
  }
}

void main() {
  late FakeMorphApi api;
  late String words;
  String? clipboard;

  setUp(() async {
    api = FakeMorphApi()..onboarded = false;
    final made = await api.createProfileIdentity('default', 'profile pass');
    words = made.words;
    clipboard = null;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, (call) async {
          if (call.method == 'Clipboard.setData') {
            clipboard = (call.arguments as Map)['text'] as String?;
          } else if (call.method == 'Clipboard.getData') {
            return {'text': clipboard};
          }
          return null;
        });
  });

  tearDown(() {
    debugDefaultTargetPlatformOverride = null;
    MorphIdentityFiles.pickBackup = () async => null;
  });

  group('restore from a backup file during onboarding', () {
    late List<MorphIdentity> done;

    Future<void> open(WidgetTester tester, List<int> file) async {
      done = [];
      MorphIdentityFiles.pickBackup = () async =>
          (name: 'backup.age', bytes: file);
      await _app(
        tester,
        MorphIdentityOnboarding(
          profileName: 'Play',
          onCreate: (pw) => api.createProfileIdentity('play', pw),
          onImport: (w, pw) => api.importProfileIdentity('play', w, pw),
          backupKind: api.identityBackupKind,
          onRestoreBackup: (contents, backupPassword, pw) =>
              api.restoreProfileBackup(
                'play',
                contents,
                backupPassword: backupPassword,
                password: pw,
              ),
          onDone: done.add,
          onSkip: () {},
          linkFromDevice: (_) => const SizedBox(),
        ),
      );
      // Offered next to "Link from another device".
      expect(find.byKey(const Key('onboarding-link')), findsOneWidget);
      await tester.tap(find.byKey(const Key('onboarding-backup')));
      await tester.pumpAndSettle();
    }

    Future<void> passwords(WidgetTester tester, String pw) async {
      await tester.enterText(find.byKey(const Key('onboarding-password')), pw);
      await tester.enterText(find.byKey(const Key('onboarding-confirm')), pw);
    }

    testWidgets('a sealed backup asks for its backup password', (tester) async {
      final backup = await api.exportProfileBackup(
        'default',
        'profile pass',
        'backup pass',
      );
      await open(tester, utf8.encode(backup));

      // Nothing chosen yet.
      await passwords(tester, 'play password');
      await tester.tap(find.byKey(const Key('onboarding-submit')));
      await tester.pumpAndSettle();
      expect(
        find.text('Choose a backup or recovery phrase file.'),
        findsOneWidget,
      );

      await tester.tap(find.byKey(const Key('onboarding-backup-choose')));
      await tester.pumpAndSettle();
      expect(find.text('backup.age'), findsOneWidget);
      expect(find.byKey(const Key('onboarding-backup-kind')), findsOneWidget);
      _expectEyes(tester, [
        'onboarding-backup-password',
        'onboarding-password',
        'onboarding-confirm',
      ]);

      await tester.tap(find.byKey(const Key('onboarding-submit')));
      await tester.pumpAndSettle();
      expect(find.text('Enter the backup password.'), findsOneWidget);

      await tester.enterText(
        find.byKey(const Key('onboarding-backup-password')),
        'not the one',
      );
      await tester.tap(find.byKey(const Key('onboarding-submit')));
      await tester.pumpAndSettle();
      expect(
        find.text('Wrong backup password, or the backup is damaged.'),
        findsOneWidget,
      );
      expect(done, isEmpty);
      expect(api.identitiesByProfile.containsKey('play'), isFalse);

      await tester.enterText(
        find.byKey(const Key('onboarding-backup-password')),
        'backup pass',
      );
      await tester.tap(find.byKey(const Key('onboarding-submit')));
      await tester.pumpAndSettle();
      expect(done.single.imported, isTrue);
      expect(
        await api.profileRecoveryWords('play', 'play password'),
        words,
        reason: 'the same identity, under the new profile password',
      );
    });

    testWidgets('a phrase file needs no backup password', (tester) async {
      final file = morphRecoveryFileText(
        profileName: 'Default',
        userId: 'u',
        words: words,
      );
      // The fake reads bare words; Rust reads Morph's phrase file too.
      await open(tester, utf8.encode(words));
      await tester.tap(find.byKey(const Key('onboarding-backup-choose')));
      await tester.pumpAndSettle();
      expect(find.text('A recovery phrase file.'), findsOneWidget);
      expect(find.byKey(const Key('onboarding-backup-password')), findsNothing);
      await passwords(tester, 'play password');
      await tester.tap(find.byKey(const Key('onboarding-submit')));
      await tester.pumpAndSettle();
      expect(done, hasLength(1));
      expect(file, contains(words));
    });

    testWidgets('other files are refused before any password', (tester) async {
      await open(tester, utf8.encode('just some notes'));
      await tester.tap(find.byKey(const Key('onboarding-backup-choose')));
      await tester.pumpAndSettle();
      expect(
        find.text('backup.age is not a Morph backup or recovery phrase.'),
        findsOneWidget,
      );
      expect(find.text('Choose file'), findsOneWidget);
    });

    testWidgets('a file too large to be a backup is not read', (tester) async {
      await open(tester, const []);
      MorphIdentityFiles.pickBackup = () async =>
          throw const FormatException('too large');
      await tester.tap(find.byKey(const Key('onboarding-backup-choose')));
      await tester.pumpAndSettle();
      expect(
        find.text('That file is too large to be a Morph backup.'),
        findsOneWidget,
      );
    });
  });

  testWidgets('the recovery phrase shows after the profile password', (
    tester,
  ) async {
    await _app(
      tester,
      Builder(
        builder: (context) => Scaffold(
          body: MorphRecoveryPhraseDialog(
            profileName: 'Default',
            userId: 'user',
            reveal: (pw) => api.profileRecoveryWords('default', pw),
          ),
        ),
      ),
    );
    _expectEyes(tester, ['recovery-phrase-password']);
    expect(find.byKey(const Key('recovery-word-grid')), findsNothing);

    await tester.enterText(
      find.byKey(const Key('recovery-phrase-password')),
      'wrong',
    );
    await tester.tap(find.byKey(const Key('recovery-phrase-show')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('recovery-phrase-error')), findsOneWidget);
    expect(find.byKey(const Key('recovery-word-grid')), findsNothing);

    await tester.enterText(
      find.byKey(const Key('recovery-phrase-password')),
      'profile pass',
    );
    await tester.tap(find.byKey(const Key('recovery-phrase-show')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('recovery-word-grid')), findsOneWidget);
    expect(find.text(' 1. ${words.split(' ').first}'), findsOneWidget);

    // Desktop: Share copies.
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    await tester.tap(find.byKey(const Key('recovery-share')));
    await tester.pumpAndSettle();
    expect(clipboard, contains(words));
    debugDefaultTargetPlatformOverride = null;
    await tester.pump(MorphRecoveryPhrase.clipboardLifetime);
    expect(clipboard, isEmpty, reason: 'the copied phrase is cleared');
  });

  group('identity backup dialog', () {
    Future<void> open(WidgetTester tester) => _app(
      tester,
      Scaffold(
        body: MorphIdentityBackupDialog(
          profileName: 'My Default',
          export: (pw, backupPw) =>
              api.exportProfileBackup('default', pw, backupPw),
        ),
      ),
    );

    Future<void> fill(
      WidgetTester tester,
      String pw,
      String backup,
      String confirm,
    ) async {
      await tester.enterText(
        find.byKey(const Key('backup-profile-password')),
        pw,
      );
      await tester.enterText(find.byKey(const Key('backup-password')), backup);
      await tester.enterText(find.byKey(const Key('backup-confirm')), confirm);
      await tester.tap(find.byKey(const Key('backup-create')));
      await tester.pumpAndSettle();
    }

    String error(WidgetTester tester) =>
        tester.widget<Text>(find.byKey(const Key('backup-error'))).data!;

    testWidgets('checks every password before sealing', (tester) async {
      await open(tester);
      _expectEyes(tester, [
        'backup-profile-password',
        'backup-password',
        'backup-confirm',
      ]);
      await fill(tester, '', 'backup pass', 'backup pass');
      expect(error(tester), 'Enter the profile password.');
      await fill(tester, 'profile pass', 'short', 'short');
      expect(error(tester), contains('at least 8 characters'));
      await fill(tester, 'profile pass', 'backup pass', 'backup pas');
      expect(error(tester), 'The backup passwords do not match.');
      expect(api.exportBackupCalls, 0);
      await fill(tester, 'wrong pass', 'backup pass', 'backup pass');
      expect(error(tester), 'That password does not open this identity.');
      expect(find.byKey(const Key('backup-save')), findsNothing);
    });

    testWidgets('saves the sealed backup where the dialog says', (
      tester,
    ) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.linux;
      final dir = Directory.systemTemp.createTempSync('morph-backup-');
      addTearDown(() => dir.deleteSync(recursive: true));
      String? suggested;
      final original = MorphRecoveryPhrase.pickSavePath;
      addTearDown(() => MorphRecoveryPhrase.pickSavePath = original);
      MorphRecoveryPhrase.pickSavePath = (name) async {
        suggested = name;
        return '${dir.path}/$name';
      };
      await open(tester);
      await fill(tester, 'profile pass', 'backup pass', 'backup pass');
      await tester.runAsync(() async {
        await tester.tap(find.byKey(const Key('backup-save')));
        for (
          var i = 0;
          i < 50 && !File('${dir.path}/$suggested').existsSync();
          i++
        ) {
          await Future<void>.delayed(const Duration(milliseconds: 20));
        }
        await Future<void>.delayed(const Duration(milliseconds: 50));
      });
      await tester.pumpAndSettle();
      expect(suggested, 'morph-my-default-identity-backup.age');
      final saved = await tester.runAsync(
        () => File('${dir.path}/$suggested').readAsString(),
      );
      expect(saved, startsWith(FakeMorphApi.fakeBackupTag));
      expect(
        tester.widget<Text>(find.byKey(const Key('backup-status'))).data,
        'Saved morph-my-default-identity-backup.age.',
      );

      await tester.tap(find.byKey(const Key('backup-share')));
      await tester.pumpAndSettle();
      expect(clipboard, saved, reason: 'no share sheet on desktop: copied');
      await tester.pump(MorphRecoveryPhrase.clipboardLifetime);
      debugDefaultTargetPlatformOverride = null;
    });
  });
}
