import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/api.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/library_page.dart';

import 'support/fake_morph_api.dart';

Future<List<String>> _pump(
  WidgetTester tester,
  FakeMorphApi api, {
  String profileId = 'work',
  MorphLibraryTab tab = MorphLibraryTab.bookmarks,
}) async {
  final opened = <String>[];
  await tester.pumpWidget(
    MaterialApp(
      theme: MorphTheme.dark,
      home: MorphLibraryPage(
        api: api,
        profileId: profileId,
        profileName: 'Work',
        initialTab: tab,
        onOpen: opened.add,
      ),
    ),
  );
  await tester.pumpAndSettle();
  return opened;
}

void main() {
  test('history day labels', () {
    final now = DateTime(2026, 11, 12, 9);
    expect(historyDayLabel(DateTime(2026, 11, 12, 1), now), 'Today');
    expect(historyDayLabel(DateTime(2026, 11, 11, 23), now), 'Yesterday');
    expect(historyDayLabel(DateTime(2026, 3, 4), now), 'Mar 4');
    expect(historyDayLabel(DateTime(2025, 12, 31), now), 'Dec 31, 2025');
  });

  testWidgets('shows only the given profile bookmarks and opens one', (
    tester,
  ) async {
    final api = FakeMorphApi();
    await api.addBookmark('work', 'https://work.test/', 'Work site');
    await api.addBookmark('default', 'https://home.test/', 'Home site');
    final opened = await _pump(tester, api);

    expect(find.text('Work site'), findsOneWidget);
    expect(find.text('Home site'), findsNothing);
    await tester.tap(find.text('Work site'));
    await tester.pumpAndSettle();
    expect(opened, ['https://work.test/']);
  });

  testWidgets('removes a bookmark', (tester) async {
    final api = FakeMorphApi();
    await api.addBookmark('work', 'https://work.test/', 'Work site');
    await _pump(tester, api);

    await tester.tap(find.byTooltip('Remove bookmark'));
    await tester.pumpAndSettle();
    expect(find.text('Work site'), findsNothing);
    expect(find.text('No bookmarks in this profile yet.'), findsOneWidget);
    expect(await api.listBookmarks('work'), isEmpty);
  });

  testWidgets('searches, deletes and clears history', (tester) async {
    final api = FakeMorphApi();
    await api.recordVisit('work', 'https://a.test/', 'Alpha');
    await api.recordVisit('work', 'https://b.test/', 'Beta');
    await api.recordVisit('default', 'https://c.test/', 'Gamma');
    await _pump(tester, api, tab: MorphLibraryTab.history);

    expect(find.text('Alpha'), findsOneWidget);
    expect(find.text('Beta'), findsOneWidget);
    expect(find.text('Gamma'), findsNothing);
    expect(find.text('Today'), findsOneWidget);

    await tester.enterText(
      find.byKey(const Key('library-history-search')),
      'alp',
    );
    await tester.pumpAndSettle();
    expect(find.text('Beta'), findsNothing);
    expect(find.text('Alpha'), findsOneWidget);

    await tester.tap(find.byTooltip('Delete from history'));
    await tester.pumpAndSettle();
    expect(find.text('Alpha'), findsNothing);

    await tester.enterText(find.byKey(const Key('library-history-search')), '');
    await tester.pumpAndSettle();
    expect(find.text('Beta'), findsOneWidget);

    await tester.tap(find.byKey(const Key('library-clear-history')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('clear-history-confirm')));
    await tester.pumpAndSettle();
    expect(find.text('No history here.'), findsOneWidget);
    expect(await api.listHistory('work'), isEmpty);
    expect(
      (await api.listHistory('default')).map((MorphHistoryEntry e) => e.url),
      ['https://c.test/'],
    );
  });
}
