import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/settings_page.dart';
import 'package:morph/src/widgets/sync_types.dart';

import 'support/fake_morph_api.dart';

Widget _section(FakeMorphApi api) => MaterialApp(
  theme: MorphTheme.dark,
  home: Scaffold(
    body: MorphSyncTypesSection(source: api, profileId: 'p1'),
  ),
);

void main() {
  testWidgets('rows show each type and toggling one calls through', (
    tester,
  ) async {
    final api = FakeMorphApi();
    await tester.pumpWidget(_section(api));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('sync-type-bookmarks')), findsOneWidget);
    expect(find.byKey(const Key('sync-type-history')), findsOneWidget);
    expect(find.text('Bookmarks'), findsOneWidget);
    expect(find.text('Folders and tags go with them.'), findsOneWidget);

    await tester.tap(find.byKey(const Key('sync-type-history')));
    await tester.pumpAndSettle();
    expect(api.syncTypeChanges, ['history=off']);
    expect(api.syncTypeList.where((t) => t.id == 'history').single.on, isFalse);
    expect(
      api.syncTypeList.where((t) => t.id == 'bookmarks').single.on,
      isTrue,
    );
  });

  testWidgets('an empty list shows how to turn sync on', (tester) async {
    final api = FakeMorphApi()..syncTypeList = const [];
    await tester.pumpWidget(_section(api));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('sync-types-empty')), findsOneWidget);
    expect(find.textContaining('Unlock the library'), findsOneWidget);
  });

  testWidgets('settings shows Sync for the profile', (tester) async {
    final api = FakeMorphApi();
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: MorphSettingsPage(
          profileName: 'Work',
          coreVersion: '0.1.0',
          renderPath: 'fake',
          onClearHistory: () async {},
          onOpenLibrary: (_) {},
          devices: api,
          profileId: 'p1',
        ),
      ),
    );
    await tester.binding.setSurfaceSize(const Size(800, 2000));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpAndSettle();
    expect(find.text('SYNC · WORK'), findsOneWidget);
    expect(find.byKey(const Key('sync-types')), findsOneWidget);
    await tester.scrollUntilVisible(find.text('LINKED DEVICES · WORK'), 200);
    expect(find.text('LINKED DEVICES · WORK'), findsOneWidget);
  });
}
