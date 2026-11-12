import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/identity_onboarding.dart';

import 'support/fake_morph_api.dart';

Future<({List<MorphIdentity> done, List<bool> skipped})> _pump(
  WidgetTester tester,
  FakeMorphApi api,
) async {
  final done = <MorphIdentity>[];
  final skipped = <bool>[];
  tester.view.physicalSize = const Size(412, 1400);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    MaterialApp(
      theme: MorphTheme.dark,
      home: MorphIdentityOnboarding(
        profileName: 'Work',
        onCreate: (pw) => api.createProfileIdentity('work', pw),
        onImport: (w, pw) => api.importProfileIdentity('work', w, pw),
        onDone: done.add,
        onSkip: () => skipped.add(true),
      ),
    ),
  );
  return (done: done, skipped: skipped);
}

void main() {
  testWidgets('create checks the password, shows 24 words, needs a tick', (
    tester,
  ) async {
    final api = FakeMorphApi()..onboarded = false;
    final r = await _pump(tester, api);
    expect(find.text('Give Work its own identity'), findsOneWidget);

    await tester.tap(find.byKey(const Key('onboarding-create')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('onboarding-password')),
      'short',
    );
    await tester.enterText(
      find.byKey(const Key('onboarding-confirm')),
      'short',
    );
    await tester.tap(find.byKey(const Key('onboarding-submit')));
    await tester.pumpAndSettle();
    expect(find.text('Use at least 8 characters.'), findsOneWidget);

    await tester.enterText(
      find.byKey(const Key('onboarding-password')),
      'long enough',
    );
    await tester.enterText(
      find.byKey(const Key('onboarding-confirm')),
      'long enougH',
    );
    await tester.tap(find.byKey(const Key('onboarding-submit')));
    await tester.pumpAndSettle();
    expect(find.text('The passwords do not match.'), findsOneWidget);
    expect(api.identitiesByProfile, isEmpty);

    await tester.enterText(
      find.byKey(const Key('onboarding-confirm')),
      'long enough',
    );
    await tester.tap(find.byKey(const Key('onboarding-submit')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('onboarding-word-grid')), findsOneWidget);
    expect(find.textContaining('24. word23'), findsOneWidget);
    final done = tester.widget<FilledButton>(
      find.byKey(const Key('onboarding-done')),
    );
    expect(done.onPressed, isNull);

    await tester.tap(find.byKey(const Key('onboarding-saved')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('onboarding-done')));
    expect(r.done.single.userId, api.identitiesByProfile['work']!.userId);
    expect(
      await api.profileRecoveryWords('work', 'long enough'),
      FakeMorphApi.fakeWords(r.done.single.userId),
    );
  });

  testWidgets('import needs 24 words and restores that identity', (
    tester,
  ) async {
    final api = FakeMorphApi()..onboarded = false;
    final r = await _pump(tester, api);
    await tester.tap(find.byKey(const Key('onboarding-import')));
    await tester.pumpAndSettle();

    await tester.enterText(find.byKey(const Key('onboarding-words')), 'a b c');
    await tester.enterText(
      find.byKey(const Key('onboarding-password')),
      'long enough',
    );
    await tester.enterText(
      find.byKey(const Key('onboarding-confirm')),
      'long enough',
    );
    await tester.tap(find.byKey(const Key('onboarding-submit')));
    await tester.pumpAndSettle();
    expect(find.text('Enter all 24 recovery words.'), findsOneWidget);

    await tester.enterText(
      find.byKey(const Key('onboarding-words')),
      FakeMorphApi.fakeWords('restoreduser'),
    );
    await tester.tap(find.byKey(const Key('onboarding-submit')));
    await tester.pumpAndSettle();
    expect(r.done.single.userId, 'restoreduser');
    expect(r.done.single.imported, isTrue);
  });

  testWidgets('not now skips', (tester) async {
    final r = await _pump(tester, FakeMorphApi()..onboarded = false);
    await tester.tap(find.byKey(const Key('onboarding-skip')));
    expect(r.skipped, [true]);
    expect(r.done, isEmpty);
  });
}
