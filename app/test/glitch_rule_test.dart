import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/tabs/tab_position.dart';
import 'package:morph/src/tabs/tab_set.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/brand.dart';
import 'package:morph/src/widgets/desktop_chrome.dart';
import 'package:morph/src/window/window_buttons.dart';

Widget _chrome(MorphTabPosition position) {
  final tabs = MorphTabSet()..update(1, title: 'Tab 1');
  return MaterialApp(
    theme: MorphTheme.dark,
    home: Scaffold(
      body: MorphDesktopChrome(
        tabs: tabs.tabs,
        activeId: tabs.activeId,
        position: position,
        sidebarCollapsed: false,
        buttonMode: MorphWindowButtonMode.all,
        platform: TargetPlatform.linux,
        useNativeMacButtons: true,
        navigation: const SizedBox(height: 48),
        content: const SizedBox.expand(),
        sidebarWidth: 220,
      ),
    ),
  );
}

void main() {
  for (final position in MorphTabPosition.values) {
    final side =
        position == MorphTabPosition.left || position == MorphTabPosition.right;
    testWidgets(
      'glitch rule accents are ${side ? 'hidden' : 'shown'} for $position tabs',
      (tester) async {
        tester.view.physicalSize = const Size(1280, 800);
        tester.view.devicePixelRatio = 1.0;
        addTearDown(tester.view.reset);
        await tester.pumpWidget(_chrome(position));
        final rule = tester.widget<MorphGlitchRule>(
          find.byKey(const Key('glitch-rule')),
        );
        expect(rule.accents, !side);
      },
    );
  }
}
