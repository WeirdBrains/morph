import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/default_browser.dart';
import 'package:morph/src/tabs/tab_position.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/library_page.dart';
import 'package:morph/src/widgets/settings_page.dart';

class _FakeBrowser extends MorphDefaultBrowser {
  _FakeBrowser(this.state, this.outcome);

  MorphDefaultBrowserState state;
  final MorphDefaultBrowserRequest outcome;
  int requests = 0;

  @override
  Future<MorphDefaultBrowserState> status() async => state;

  @override
  Future<MorphDefaultBrowserRequest> makeDefault() async {
    requests++;
    if (outcome == MorphDefaultBrowserRequest.madeDefault) {
      state = MorphDefaultBrowserState.isDefault;
    }
    return outcome;
  }
}

Widget _settings(MorphDefaultBrowser? browser) => MaterialApp(
  theme: MorphTheme.dark,
  home: MorphSettingsPage(
    profileName: 'Work',
    coreVersion: '0.1.0',
    renderPath: 'fake',
    onClearHistory: () async {},
    onOpenLibrary: (_) {},
    defaultBrowser: browser,
  ),
);

void main() {
  testWidgets('Make default asks the system and shows the new status', (
    tester,
  ) async {
    final browser = _FakeBrowser(
      MorphDefaultBrowserState.notDefault,
      MorphDefaultBrowserRequest.madeDefault,
    );
    await tester.pumpWidget(_settings(browser));
    await tester.pumpAndSettle();
    expect(find.text('Another browser opens your links'), findsOneWidget);
    await tester.tap(find.byKey(const Key('settings-make-default')));
    await tester.pumpAndSettle();
    expect(browser.requests, 1);
    expect(find.text('Morph is now your default browser'), findsOneWidget);
    expect(find.text('Morph opens your links'), findsOneWidget);
    expect(find.byKey(const Key('settings-make-default')), findsNothing);
  });

  testWidgets('settings that open the system screen say so', (tester) async {
    final browser = _FakeBrowser(
      MorphDefaultBrowserState.notDefault,
      MorphDefaultBrowserRequest.openedSettings,
    );
    await tester.pumpWidget(_settings(browser));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('settings-make-default')));
    await tester.pumpAndSettle();
    expect(
      find.text('Pick Morph as the web browser in the system settings'),
      findsOneWidget,
    );
    expect(find.byKey(const Key('settings-make-default')), findsOneWidget);
  });

  testWidgets('no default-browser support hides the section', (tester) async {
    await tester.pumpWidget(_settings(null));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('settings-default-browser')), findsNothing);
  });

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
