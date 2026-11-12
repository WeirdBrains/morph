import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/tabs/tab_position.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/library_page.dart';
import 'package:morph/src/widgets/settings_page.dart';

void main() {
  testWidgets('desktop settings change layout and clear only this profile', (
    tester,
  ) async {
    var cleared = 0;
    final positions = <MorphTabPosition>[];
    final collapsed = <bool>[];
    final opened = <MorphLibraryTab>[];
    await tester.pumpWidget(
      MaterialApp(
        theme: MorphTheme.dark,
        home: MorphSettingsPage(
          profileName: 'Work',
          coreVersion: '0.1.0',
          renderPath: 'webkitgtk-snapshot',
          tabPosition: MorphTabPosition.top,
          sidebarCollapsed: false,
          onClearHistory: () async => cleared++,
          onOpenLibrary: opened.add,
          onTabPositionChanged: positions.add,
          onSidebarCollapsedChanged: collapsed.add,
        ),
      ),
    );
    expect(find.text('PROFILE · WORK'), findsOneWidget);
    expect(find.text('core 0.1.0'), findsOneWidget);

    await tester.tap(find.byKey(const Key('settings-tab-position')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Left').last);
    await tester.pumpAndSettle();
    expect(positions, [MorphTabPosition.left]);

    await tester.tap(find.byKey(const Key('settings-sidebar-collapsed')));
    await tester.pump();
    expect(collapsed, [true]);

    await tester.tap(find.byKey(const Key('settings-history')));
    expect(opened, [MorphLibraryTab.history]);

    await tester.tap(find.byKey(const Key('settings-clear-history')));
    await tester.pumpAndSettle();
    expect(
      find.text('This removes every visit saved in Work.'),
      findsOneWidget,
    );
    await tester.tap(find.byKey(const Key('clear-history-confirm')));
    await tester.pumpAndSettle();
    expect(cleared, 1);
    expect(find.text('History cleared in Work'), findsOneWidget);
  });

  testWidgets('phone settings hide the desktop layout section', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: MorphSettingsPage(
          profileName: 'Default',
          coreVersion: '0.1.0',
          renderPath: 'android-platform-view',
          onClearHistory: () async {},
          onOpenLibrary: (_) {},
        ),
      ),
    );
    expect(find.byKey(const Key('settings-tab-position')), findsNothing);
    expect(find.byKey(const Key('settings-clear-history')), findsOneWidget);
  });
}
