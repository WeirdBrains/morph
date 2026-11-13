// Identity onboarding and the identity dialogs in short desktop windows (a
// tiled window can be well under the 480 px Morph asks for): nothing may
// overflow, and every control stays reachable by scrolling.
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/app.dart';
import 'package:morph/src/shell.dart';
import 'package:morph_webview/morph_webview.dart';

import 'support/fake_morph_api.dart';

Future<void> _tap(WidgetTester tester, String key) async {
  final finder = find.byKey(Key(key));
  await tester.ensureVisible(finder);
  await tester.pumpAndSettle();
  await tester.tap(finder);
  await tester.pumpAndSettle();
}

Future<void> _type(WidgetTester tester, String key, String text) async {
  final finder = find.byKey(Key(key));
  await tester.ensureVisible(finder);
  await tester.pumpAndSettle();
  await tester.enterText(finder, text);
  await tester.pump();
}

void main() {
  setUp(() {
    MorphWebViewPlatform.instance = FakeMorphWebViewPlatform();
  });

  for (final size in const [Size(1024, 469), Size(800, 360), Size(640, 300)]) {
    testWidgets(
      'identity flow fits a ${size.width.toInt()}x${size.height.toInt()} window',
      (tester) async {
        debugDefaultTargetPlatformOverride = TargetPlatform.linux;
        tester.view.physicalSize = size;
        tester.view.devicePixelRatio = 1.0;
        final api = FakeMorphApi()..onboarded = false;
        try {
          await tester.pumpWidget(
            MorphApp(
              api: api,
              home: '/tmp/morph-does-not-exist',
              subscribeToTicks: false,
              loadPageBridgeAsset: false,
            ),
          );
          await tester.pump();
          await tester.pump();
          expect(
            tester
                .state<MorphShellState>(find.byType(MorphShell))
                .coreVersionLabel,
            isNotNull,
          );
          await tester.pumpAndSettle();

          // Every way in is reachable, "Not now" included.
          for (final key in [
            'onboarding-create',
            'onboarding-import',
            'onboarding-backup',
            'onboarding-link',
            'onboarding-skip',
          ]) {
            final finder = find.byKey(Key(key));
            expect(finder, findsOneWidget, reason: key);
            await tester.ensureVisible(finder);
            await tester.pumpAndSettle();
          }

          // The restore forms fit too; back to the welcome step after each.
          for (final (step, fields) in const [
            (
              'onboarding-import',
              [
                'onboarding-words',
                'onboarding-password',
                'onboarding-confirm',
                'onboarding-submit',
              ],
            ),
            (
              'onboarding-backup',
              [
                'onboarding-backup-choose',
                'onboarding-password',
                'onboarding-confirm',
                'onboarding-submit',
              ],
            ),
          ]) {
            await _tap(tester, step);
            for (final key in fields) {
              final finder = find.byKey(Key(key));
              expect(finder, findsOneWidget, reason: key);
              await tester.ensureVisible(finder);
              await tester.pumpAndSettle();
            }
            await tester.tap(
              find.descendant(
                of: find.byKey(const Key('identity-onboarding')),
                matching: find.byTooltip('Back'),
              ),
            );
            await tester.pumpAndSettle();
          }

          await _tap(tester, 'onboarding-create');
          await _type(tester, 'onboarding-password', 'long enough');
          await _type(tester, 'onboarding-confirm', 'long enough');
          await _tap(tester, 'onboarding-submit');
          expect(find.byKey(const Key('recovery-word-grid')), findsOneWidget);
          await _tap(tester, 'onboarding-saved');
          await _tap(tester, 'onboarding-done');

          Future<void> identityAction(String key) async {
            await _tap(tester, 'nav-menu');
            await _tap(tester, 'menu-identity');
            await _tap(tester, key);
          }

          await identityAction('identity-recovery-phrase');
          await _type(tester, 'recovery-phrase-password', 'long enough');
          await _tap(tester, 'recovery-phrase-show');
          expect(find.byKey(const Key('recovery-word-grid')), findsOneWidget);
          await _tap(tester, 'recovery-phrase-close');

          await identityAction('identity-backup');
          await _type(tester, 'backup-profile-password', 'long enough');
          await _type(tester, 'backup-password', 'backup pass');
          await _type(tester, 'backup-confirm', 'backup pass');
          await _tap(tester, 'backup-create');
          expect(find.byKey(const Key('backup-save')), findsOneWidget);
          await _tap(tester, 'backup-close');
          expect(find.byKey(const Key('identity-backup-dialog')), findsNothing);
        } finally {
          debugDefaultTargetPlatformOverride = null;
          tester.view.reset();
        }
      },
    );
  }
}
