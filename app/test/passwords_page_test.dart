import 'package:material_ui/material_ui.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/passwords_page.dart';

import 'support/fake_morph_api.dart';

Widget _page(FakeMorphApi api) => MaterialApp(
  theme: MorphTheme.dark,
  home: MorphPasswordsPage(api: api, profileId: 'default'),
);

void main() {
  testWidgets('a profile without an identity cannot save passwords', (
    tester,
  ) async {
    final api = FakeMorphApi()..onboarded = false;
    await tester.pumpWidget(_page(api));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('passwords-unavailable')), findsOneWidget);
    expect(find.textContaining('no identity'), findsOneWidget);
    expect(find.byKey(const Key('passwords-enabled')), findsNothing);
  });

  testWidgets(
    'a profile with an identity has the saver on and can turn it off',
    (tester) async {
      final api = FakeMorphApi();
      await tester.pumpWidget(_page(api));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('passwords-enabled')), findsOneWidget);
      final enabled = tester.widget<SwitchListTile>(
        find.byKey(const Key('passwords-enabled')),
      );
      expect(enabled.value, isTrue);
      await tester.tap(find.byKey(const Key('passwords-enabled')));
      await tester.pumpAndSettle();
      expect(api.saverEnabled['default'], isFalse);
      expect(
        find.text(
          'Morph no longer offers to save or fill passwords. Saved ones stay.',
        ),
        findsOneWidget,
      );
    },
  );

  testWidgets(
    'saved logins can be shown after the profile password and deleted',
    (tester) async {
      final api = FakeMorphApi()
        ..libraryPasswords['default'] = 'long enough'
        ..savedLogins['default'] = [
          const MorphSavedLogin(
            id: 'l1',
            origin: 'https://example.com',
            username: 'amy',
          ),
        ]
        ..savedPasswords['l1'] = 's3cret';
      await tester.pumpWidget(_page(api));
      await tester.pumpAndSettle();
      expect(find.text('amy · https://example.com'), findsOneWidget);

      await tester.tap(find.byKey(const Key('passwords-show-l1')));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const Key('passwords-profile-password')),
        'long enough',
      );
      await tester.tap(find.byKey(const Key('passwords-password-ok')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('passwords-secret-l1')), findsOneWidget);
      expect(find.text('s3cret'), findsOneWidget);

      await tester.tap(find.byKey(const Key('passwords-delete-l1')));
      await tester.pumpAndSettle();
      expect(find.text('Delete?'), findsOneWidget);
      await tester.tap(find.byKey(const Key('passwords-delete-l1')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('passwords-empty')), findsOneWidget);
      expect(api.savedLogins['default'], isEmpty);
    },
  );
}
